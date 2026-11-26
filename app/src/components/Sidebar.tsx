import { useEffect, useMemo, useRef, useState } from "react";
import {
  Command as CommandIcon,
  Ellipsis,
  Folder,
  FolderOpen,
  FolderPlus,
  LayoutGrid,
  Plus,
  Search,
  Settings,
  SquareTerminal,
  X,
} from "lucide-react";

import { useT } from "../i18n";
import { api, type Diff, type Snapshot, type Task } from "../lib/api";
import { elapsed, writerOf } from "../lib/format";
import type { View } from "../App";
import { AgentIcon } from "./AgentIcon";
import { StateIcon, StatusIcon, useStateLabel } from "./StatusIcon";
import { ContextMenu, type MenuEntry } from "./ContextMenu";
import { SettingsPopover } from "./SettingsPopover";
import { isMac, kbd, modKey } from "../lib/platform";
import { beginPaneDrag } from "../lib/paneDrag";

interface Props {
  snap: Snapshot | null;
  view: View;
  /** 每次变化时把焦点放到搜索框（⌘K）。 */
  focusSearch: number;
  onNewTask(project?: string): void;
  onOpenTask(task: number): void;
  /** 并排在右侧的任务（不并排时为空）。 */
  panes: number[];
  /** 并排看这个项目的全部任务。 */
  onShowProject(project: string): void;
  /** 把任务从并排的格子里拿掉（任务不关）。 */
  onClosePane(task: number): void;
  /** 打开任务，在它下方开一个终端。 */
  onOpenShell(task: number): void;
  /** 打开任务，选中它下方的某个终端。 */
  onOpenShellTab(task: number, agent: number): void;
  /** 放在旁边一格（⌘ 点任务）。 */
  onOpenBeside(task: number): void;
  onOpenReview(task: number): void;
  /** 交给另一个 Agent：审阅、接力或分出新任务。 */
  onHandoff(task: number): void;
  onCloseTask(task: number): void;
  onAddProject(): void;
  onJumpToWaiting(): void;
  /** ⌘K 搜索框里也能搜到的命令。 */
  commands: SearchCommand[];
  onError(err: unknown): void;
}

/** 搜索框里能搜到的一条命令：名字、快捷键（macOS 写法）和执行。 */
export interface SearchCommand {
  id: string;
  label: string;
  keys?: string;
  run(): void;
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
  const commands = needle ? props.commands.filter((c) => c.label.toLowerCase().includes(needle)) : [];
  const runCommand = (command: SearchCommand) => {
    setQuery("");
    search.current?.blur();
    command.run();
  };

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
              } else if (e.key === "Enter" && !e.nativeEvent.isComposing && needle) {
                // 回车：打开第一个找到的任务，没有任务时执行第一条命令。
                const task = projects.flatMap((p) => p.tasks).find(matches);
                if (task) {
                  setQuery("");
                  e.currentTarget.blur();
                  props.onOpenTask(task.id);
                } else if (commands[0]) runCommand(commands[0]);
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
        {commands.length > 0 && (
          <section className="side-commands">
            {commands.map((command) => (
              <button key={command.id} className="side-command" onClick={() => runCommand(command)}>
                <CommandIcon size={13} strokeWidth={1.75} className="dim" />
                <span className="grow ellipsis">{command.label}</span>
                {command.keys && <kbd>{kbd(command.keys)}</kbd>}
              </button>
            ))}
          </section>
        )}
        {projects.map((project) => {
          const tasks = project.tasks.filter(matches);
          if (needle && tasks.length === 0) return null;
          const folded = !needle && collapsed.has(project.id);
          return (
            <section key={project.id} className="side-project">
              <div className="side-project-head">
                {/* 项目是分组标题：文件夹图标（合上 / 打开就是收起 / 展开）加上比任务名醒目的名字。 */}
                <button className="project-toggle" onClick={() => toggleCollapsed(project.id)} title={project.root}>
                  {folded ? <Folder {...ICON} className="project-icon" /> : <FolderOpen {...ICON} className="project-icon" />}
                  <span className="ellipsis">{project.name}</span>
                </button>
                {folded && project.tasks.length > 0 && <FoldedCounts tasks={project.tasks} />}
                {project.tasks.length >= 2 && (
                  <button
                    className="icon-btn project-grid"
                    title={t("sidebar.showProject")}
                    onClick={() => props.onShowProject(project.id)}
                  >
                    <LayoutGrid {...ICON} />
                  </button>
                )}
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
  panes,
  onOpenTask,
  onOpenBeside,
  onOpenReview,
  onHandoff,
  onClosePane,
  onOpenShell,
  onOpenShellTab,
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
    panes.includes(task.id)
      ? { label: t("task.unsplit"), run: () => onClosePane(task.id) }
      : { label: t("sidebar.openBeside"), kbd: "⌘\\", run: () => onOpenBeside(task.id) },
    { label: t("sidebar.rename"), run: () => setRenaming(true) },
    { label: t("sidebar.handoff"), run: () => onHandoff(task.id) },
    { sep: true },
    { label: t(isMac ? "task.reveal" : "task.openFolder"), run: () => void api.reveal(task.worktree) },
    { label: t("shell.openHere"), run: () => onOpenShell(task.id) },
    { label: t("task.openTerminal"), run: () => void api.openTerminal(task.worktree) },
    { label: t("sidebar.copyBranch"), run: () => void navigator.clipboard.writeText(task.branch) },
    { sep: true },
    { label: t("task.close"), kbd: "⌘⇧W", danger: true, run: () => onCloseTask(task.id) },
  ];

  const shells = task.agents.filter((a) => a.role === "shell");
  return (
    <>
    <div
      className={`task-row ${current ? "is-current" : ""} ${panes.includes(task.id) && !current ? "is-beside" : ""} ${status === "needs_input" ? "is-waiting" : ""}`}
      role="button"
      tabIndex={0}
      // ⌘ 点：放在旁边一格，两个任务并排看。
      onClick={(e) => (modKey(e.nativeEvent) ? onOpenBeside(task.id) : onOpenTask(task.id))}
      // 按住拖到右侧：并排看（见 lib/paneDrag.ts）。
      onPointerDown={(e) => {
        if (!renaming && !(e.target as Element).closest("button, input")) beginPaneDrag(e, task.id, title);
      }}
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
        {/* 空白任务也画状态（淡一些）：这一列空着，看起来像缩进在上一个任务下面。 */}
        <span className={`row-status ${untitled && task.state !== "needs_you" ? "is-faded" : ""}`}>
          <StateIcon task={task} />
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
    {/* 任务里另开的终端：挂在任务下面，点了打开任务并选中它。 */}
    {shells.map((shell, i) => (
      <div
        key={shell.id}
        className="task-row shell-row"
        role="button"
        tabIndex={0}
        title={t("shell.tab", { n: i + 1 })}
        onClick={() => onOpenShellTab(task.id, shell.id)}
        onKeyDown={(e) => e.key === "Enter" && onOpenShellTab(task.id, shell.id)}
      >
        <div className="task-line">
          <StatusIcon status={shell.status} />
          <SquareTerminal {...ICON} className="dim" />
          <span className="grow ellipsis">{t("shell.tab", { n: i + 1 })}</span>
          <span className="row-actions">
            <button
              className="icon-btn"
              title={t("shell.close")}
              onClick={(e) => {
                e.stopPropagation();
                api.closeShell(task.id, shell.id).catch(onError);
              }}
            >
              <X {...ICON} />
            </button>
          </span>
        </div>
      </div>
    ))}
    </>
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

/** 收起的项目：里面有几个在等你、几个在干活，再加任务总数。 */
function FoldedCounts({ tasks }: { tasks: Task[] }) {
  const t = useT();
  const waiting = tasks.filter((task) => task.state === "needs_you").length;
  const running = tasks.filter((task) => task.state === "writing" || task.state === "reviewing").length;
  return (
    <span className="folded-counts">
      {waiting > 0 && (
        <span className="is-waiting" title={t("sidebar.waiting", { count: waiting })}>
          <span className="attention-dot" />
          {waiting}
        </span>
      )}
      {running > 0 && (
        <span title={t("sidebar.running", { count: running })}>
          <span className="status status-running" />
          {running}
        </span>
      )}
      <span className="meta" title={t("sidebar.taskCount", { count: tasks.length })}>
        {tasks.length}
      </span>
    </span>
  );
}
