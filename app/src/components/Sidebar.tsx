import { useEffect, useMemo, useRef, useState } from "react";
import {
  ChevronDown,
  ChevronRight,
  Ellipsis,
  FolderPlus,
  Plus,
  Search,
  Settings,
  X,
} from "lucide-react";

import { useT } from "../i18n";
import { api, type Diff, type Snapshot, type Task } from "../lib/api";
import { elapsed, writerOf } from "../lib/format";
import type { View } from "../App";
import { AgentIcon } from "./AgentIcon";
import { StateIcon, useStateLabel } from "./StatusIcon";
import { ContextMenu, type MenuEntry } from "./ContextMenu";
import { SettingsPopover } from "./SettingsPopover";
import { isMac, kbd } from "../lib/platform";

interface Props {
  snap: Snapshot | null;
  view: View;
  /** 每次变化时把焦点放到搜索框（⌘K）。 */
  focusSearch: number;
  onNewTask(project?: string): void;
  onOpenTask(task: number): void;
  onOpenReview(task: number): void;
  onStartReview(task: number): void;
  onCloseTask(task: number): void;
  onAddProject(): void;
  onJumpToWaiting(): void;
  onError(err: unknown): void;
}

const MIN_WIDTH = 220;
const MAX_WIDTH = 440;
const WIDTH_KEY = "duolo.sidebarWidth";
const COLLAPSED_KEY = "duolo.collapsedProjects";
const ICON = { size: 14, strokeWidth: 1.75 } as const;

export function Sidebar(props: Props) {
  const { snap, view, onNewTask, onAddProject, onJumpToWaiting } = props;
  const t = useT();
  const [width, startResize] = useResizableWidth();
  const [query, setQuery] = useState("");
  const [collapsed, toggleCollapsed] = useCollapsed();
  const [settingsOpen, setSettingsOpen] = useState(false);
  const search = useRef<HTMLInputElement>(null);
  const projects = snap?.projects ?? [];
  const waiting = projects.flatMap((p) => p.tasks).filter((task) => writerOf(task)?.status === "needs_input").length;

  useEffect(() => {
    if (props.focusSearch) search.current?.focus();
  }, [props.focusSearch]);

  const needle = query.trim().toLowerCase();
  const matches = (task: Task) =>
    !needle || task.title.toLowerCase().includes(needle) || task.branch.toLowerCase().includes(needle);

  return (
    <aside className="sidebar" style={{ width }}>
      <div className="sidebar-top" data-tauri-drag-region />
      <div className="side-search">
        <label className="search-box">
          <Search {...ICON} />
          <input
            ref={search}
            value={query}
            placeholder={t("sidebar.search")}
            aria-label={t("sidebar.search")}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Escape") {
                setQuery("");
                e.currentTarget.blur();
              }
            }}
          />
          {!query && <kbd>{kbd("⌘K")}</kbd>}
        </label>
        <button className="square-btn" title={t("sidebar.addProjectShortcut")} onClick={onAddProject}>
          <FolderPlus {...ICON} />
        </button>
      </div>

      <div className="side-scroll">
        {waiting > 0 && !needle && (
          <button className="attention" onClick={onJumpToWaiting} title={kbd("⌥↓")}>
            <span className="attention-dot" />
            <span className="grow">{t("sidebar.waiting", { count: waiting })}</span>
            <kbd>{kbd("⌥↓")}</kbd>
          </button>
        )}
        {projects.map((project) => {
          const tasks = project.tasks.filter(matches);
          if (needle && tasks.length === 0) return null;
          const folded = !needle && collapsed.has(project.id);
          return (
            <section key={project.id} className="side-project">
              <div className="side-project-head">
                <button className="project-toggle" onClick={() => toggleCollapsed(project.id)} title={project.root}>
                  {folded ? <ChevronRight size={12} strokeWidth={2} /> : <ChevronDown size={12} strokeWidth={2} />}
                  <span className="ellipsis">{project.name}</span>
                </button>
                {folded && project.tasks.length > 0 && <span className="meta">{project.tasks.length}</span>}
                <button
                  className="icon-btn project-add"
                  title={t("sidebar.newTaskIn", { project: project.name })}
                  onClick={() => onNewTask(project.id)}
                >
                  <Plus {...ICON} />
                </button>
              </div>
              {!folded &&
                tasks.map((task) => (
                  <TaskRow
                    key={task.id}
                    task={task}
                    current={view.kind === "task" && view.task === task.id}
                    {...props}
                  />
                ))}
            </section>
          );
        })}
        {snap && projects.length === 0 && <div className="side-empty">{t("sidebar.noProjects")}</div>}
      </div>

      <footer className="sidebar-foot">
        <span className="grow foot-stats">
          {snap && snap.running > 0 && (
            <span title={t("sidebar.running", { count: snap.running })}>
              <span className="status status-running" />
              {snap.running}
            </span>
          )}
        </span>
        <div className="settings-anchor">
          <button className="icon-btn" title={t("settings.title")} onClick={() => setSettingsOpen((o) => !o)}>
            <Settings {...ICON} />
          </button>
          {settingsOpen && <SettingsPopover onClose={() => setSettingsOpen(false)} onError={props.onError} />}
        </div>
      </footer>
      <div className="sidebar-resizer" onMouseDown={startResize} onDoubleClick={() => startResize(null)} />
    </aside>
  );
}

/** 一个任务一行：状态、模型图标、标题、审阅的小标记。当前任务多一行：分支、改动、时长。 */
function TaskRow({
  task,
  current,
  onOpenTask,
  onOpenReview,
  onStartReview,
  onCloseTask,
  onError,
}: Props & { task: Task; current: boolean }) {
  const t = useT();
  const writer = writerOf(task);
  const status = writer?.status ?? "not_started";
  const stateLabel = useStateLabel()(task);
  const untitled = task.untitled;
  const title = untitled ? t("task.untitled") : task.title;
  const [renaming, setRenaming] = useState(false);
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  const diff = useDiff(current ? task.id : null);

  const entries: MenuEntry[] = [
    { label: t("sidebar.rename"), run: () => setRenaming(true) },
    { label: t("sidebar.review", { agent: task.reviewer.name }), kbd: "⌘⇧R", run: () => onStartReview(task.id) },
    { sep: true },
    { label: t(isMac ? "task.reveal" : "task.openFolder"), run: () => void api.reveal(task.worktree) },
    { label: t("task.openTerminal"), run: () => void api.openTerminal(task.worktree) },
    { label: t("sidebar.copyBranch"), run: () => void navigator.clipboard.writeText(task.branch) },
    { sep: true },
    { label: t("task.close"), kbd: "⌘⇧W", danger: true, run: () => onCloseTask(task.id) },
  ];

  return (
    <div
      className={`task-row ${current ? "is-current" : ""} ${status === "needs_input" ? "is-waiting" : ""}`}
      role="button"
      tabIndex={0}
      onClick={() => onOpenTask(task.id)}
      onDoubleClick={() => setRenaming(true)}
      onKeyDown={(e) => e.key === "Enter" && !renaming && onOpenTask(task.id)}
      onContextMenu={(e) => {
        e.preventDefault();
        setMenu({ x: e.clientX, y: e.clientY });
      }}
      title={writer ? `${title}\n${writer.name} · ${stateLabel}` : title}
    >
      <div className="task-line">
        {/* 状态在行首，和手机端、GitHub 的列表一样；名字在悬停提示里。 */}
        <span className="row-status">
          {untitled && task.state !== "needs_you" ? null : <StateIcon task={task} />}
        </span>
        <span className={untitled ? "is-faded" : ""}>
          <AgentIcon kind={writer?.kind ?? ""} title={writer?.name} />
        </span>
        {renaming ? (
          <RenameInput
            initial={untitled ? "" : task.title}
            onDone={(next) => {
              setRenaming(false);
              if (next && next !== task.title) api.renameTask(task.id, next).catch(onError);
            }}
          />
        ) : (
          <span className={`grow ellipsis task-title ${untitled ? "untitled" : ""}`}>{title}</span>
        )}
        {task.repo && <span className="repo-tag">{task.repo}</span>}
        <ReviewChip task={task} onOpen={() => onOpenReview(task.id)} />
        <span className="row-actions">
          <button
            className="icon-btn"
            title={t("common.more")}
            onClick={(e) => {
              e.stopPropagation();
              const r = e.currentTarget.getBoundingClientRect();
              setMenu({ x: r.left, y: r.bottom + 4 });
            }}
          >
            <Ellipsis {...ICON} />
          </button>
          <button
            className="icon-btn"
            title={t("sidebar.closeTask")}
            onClick={(e) => {
              e.stopPropagation();
              onCloseTask(task.id);
            }}
          >
            <X {...ICON} />
          </button>
        </span>
      </div>
      {current && (
        <div className="task-meta">
          <span className="ellipsis">{task.branch.replace(/^duolo\//, "")}</span>
          {diff && diff.files > 0 && (
            <span>
              <span className="ins">+{diff.insertions}</span> <span className="del">−{diff.deletions}</span>
            </span>
          )}
          <span className="grow" />
          <span>{elapsed(task.createdAt)}</span>
        </div>
      )}
      {menu && <ContextMenu at={menu} entries={entries} onDismiss={() => setMenu(null)} />}
    </div>
  );
}

/** 打开审阅的小标记（审阅中、审过才有）。结论由行首的状态图标表示，这里不带颜色。 */
function ReviewChip({ task, onOpen }: { task: Task; onOpen(): void }) {
  const t = useT();
  const running = task.reviewingSince != null;
  if (!running && !task.review) return null;
  const label = running ? t("sidebar.reviewRunning") : task.review?.ok ? task.review.summary : t("sidebar.reviewFailed");
  return (
    <button
      className="review-chip"
      title={`${t("sidebar.reviewTitle", { agent: task.reviewer.name })} · ${label}`}
      onClick={(e) => {
        e.stopPropagation();
        onOpen();
      }}
    >
      <AgentIcon kind={task.reviewer.kind} size={11} />
    </button>
  );
}

function RenameInput({ initial, onDone }: { initial: string; onDone(next: string | null): void }) {
  const [value, setValue] = useState(initial);
  const done = useRef(false);
  const finish = (next: string | null) => {
    if (done.current) return;
    done.current = true;
    onDone(next?.trim() || null);
  };
  return (
    <input
      className="rename-input grow"
      autoFocus
      value={value}
      onClick={(e) => e.stopPropagation()}
      onFocus={(e) => e.currentTarget.select()}
      onChange={(e) => setValue(e.target.value)}
      onBlur={() => finish(value)}
      onKeyDown={(e) => {
        e.stopPropagation();
        if (e.key === "Enter") finish(value);
        if (e.key === "Escape") finish(null);
      }}
    />
  );
}

/** 当前任务的改动统计，每 3 秒刷新。 */
function useDiff(task: number | null) {
  const [diff, setDiff] = useState<Diff | null>(null);
  useEffect(() => {
    setDiff(null);
    if (task == null) return;
    let alive = true;
    const load = () => api.diffStat(task).then((d) => alive && setDiff(d), () => {});
    void load();
    const id = window.setInterval(load, 3000);
    return () => {
      alive = false;
      window.clearInterval(id);
    };
  }, [task]);
  return diff;
}

/** 折叠了的项目，记在本机。 */
function useCollapsed(): [Set<string>, (id: string) => void] {
  const [set, setSet] = useState<Set<string>>(() => {
    try {
      return new Set(JSON.parse(localStorage.getItem(COLLAPSED_KEY) ?? "[]") as string[]);
    } catch {
      return new Set();
    }
  });
  const toggle = useMemo(
    () => (id: string) =>
      setSet((old) => {
        const next = new Set(old);
        if (next.has(id)) next.delete(id);
        else next.add(id);
        try {
          localStorage.setItem(COLLAPSED_KEY, JSON.stringify([...next]));
        } catch {
          // 存不了就只在本次生效。
        }
        return next;
      }),
    [],
  );
  return [set, toggle];
}

/** 侧边栏宽度：拖右边缘调整，双击恢复默认，记在本机。 */
function useResizableWidth(): [number, (e: React.MouseEvent | null) => void] {
  const DEFAULT = 272;
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
