import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";

import { errorMessage, useT } from "./i18n";
import { api, type AgentRef, type Check, type NewTaskOptions, type Snapshot } from "./lib/api";
import { allTasks, findTask, isLive, writerOf } from "./lib/format";
import * as firstPrompt from "./lib/firstPrompt";
import { disposeTask, setInputListener } from "./lib/terminals";
import { useVoice, type VoiceTarget } from "./lib/voice";
import { useProjectAdder } from "./lib/projects";
import { AddProjectDialog } from "./components/AddProject";
import { agentOptions, isInstalled, type AgentChoice } from "./lib/agents";
import { Sidebar } from "./components/Sidebar";
import { Empty } from "./components/Empty";
import { TaskView } from "./components/TaskView";
import { Confirm, type ConfirmSpec } from "./components/Confirm";
import { Notice } from "./components/Notice";
import { NewTaskPanel } from "./components/NewTaskPanel";
import { isMac, matches } from "./lib/platform";

/** 右侧显示什么：某个任务的某个 Agent，或者没有任务时的空状态。 */
/** 和原生菜单（crates/duolo-app/src/main.rs 的 build_menu）里一样的快捷键。 */
const MENU_SHORTCUTS: [string, string][] = [
  ["quit", "⌘Q"],
  ["new-task", "⌘N"],
  ["add-project", "⌘O"],
  ["review", "⌘⇧R"],
  ["prev-task", "⌘⌥↑"],
  ["next-task", "⌘⌥↓"],
  ["reveal", "⌘⇧F"],
  ["close-task", "⌘⇧W"],
];

/** 菜单里不列的快捷键：⌘1…⌘9 跳到第 n 个任务，⌘K 搜索任务，⌥↓ 跳到下一个等你确认的任务。 */
const EXTRA_SHORTCUTS: [string, string][] = [
  ...[1, 2, 3, 4, 5, 6, 7, 8, 9].map((n): [string, string] => [`task-${n}`, `⌘${n}`]),
  ["search", "⌘K"],
  ["next-waiting", "⌥↓"],
];

export type View = { kind: "empty" } | { kind: "task"; task: number };

export function App() {
  const t = useT();
  const [snap, setSnap] = useState<Snapshot | null>(null);
  const [env, setEnv] = useState<Check[] | null>(null);
  /** 配置里默认的写代码和审阅 Agent。 */
  const [agentDefaults, setAgentDefaults] = useState<AgentChoice | null>(null);
  const [view, setView] = useState<View>({ kind: "empty" });
  const [creating, setCreating] = useState(false);
  const [confirm, setConfirm] = useState<ConfirmSpec | null>(null);
  const [toast, setToast] = useState<string | null>(null);
  /** 启动时发现的问题，看过就清空。 */
  const [notices, setNotices] = useState<unknown[]>([]);
  const clearNotices = useCallback(() => setNotices([]), []);
  /** 打开了审阅面板的任务。 */
  const [reviewOpen, setReviewOpen] = useState<number | null>(null);
  /** 新建任务面板（⌘N）和它预选的项目。 */
  const [panel, setPanel] = useState<{ project: string | null } | null>(null);
  /** 正在把文件夹拖进窗口。 */
  const [dragging, setDragging] = useState(false);
  /** ⌘K：每次加一，侧边栏把焦点放到搜索框。 */
  const [focusSearch, setFocusSearch] = useState(0);
  const snapRef = useRef(snap);
  snapRef.current = snap;

  const flash = useCallback((text: string) => {
    setToast(text);
    // 长的提示（例如为什么没能关闭任务、怎么处理）多留一会儿，来得及看完。
    const stay = Math.max(4000, text.length * 60);
    window.setTimeout(() => setToast((t) => (t === text ? null : t)), stay);
  }, []);
  const fail = useCallback((err: unknown) => flash(errorMessage(t, err)), [flash, t]);

  const refresh = useCallback(async () => {
    const fresh = await api.snapshot();
    // 立刻更新，不等重新渲染：紧接着的 openTask 等要用到最新的任务列表。
    snapRef.current = fresh;
    setSnap(fresh);
    return fresh;
  }, []);

  const current = view.kind === "task" ? findTask(snap, view.task) : null;

  // ---- 打开和新建任务 ----

  const openTask = useCallback((task: number) => {
    if (findTask(snapRef.current, task)) setView({ kind: "task", task });
  }, []);

  /** 当前任务所在的项目，没有时是第一个项目。 */
  const currentProject = useCallback(() => {
    const s = snapRef.current;
    return (view.kind === "task" ? findTask(s, view.task)?.project.id : undefined) ?? s?.projects[0]?.id ?? null;
  }, [view]);

  /**
   * 新建任务并打开它的命令行。没写需求、也没换基准分支和分支名时，同一个仓库里已有的空白任务直接复用。
   * 写了需求时，Agent 启动后直接开始做。
   */
  const createTask = useCallback(
    async (project: string, prompt: string, opts: NewTaskOptions = {}) => {
      const p = snapRef.current?.projects.find((x) => x.id === project);
      // Windows 上 git 给的路径用 /，其他的用 \\，统一了再比。
      const norm = (path?: string) => path?.replace(/\\/g, "/");
      const sameRepo = (repo: string) => !opts.repo || norm(opts.repo) === norm(repo ? `${p?.root}/${repo}` : p?.root);
      const blank = p?.tasks.find(
        (t) => t.untitled && sameRepo(t.repo) && (!opts.writer || writerOf(t)?.kind === opts.writer),
      );
      setPanel(null);
      if (!prompt && !opts.base && !opts.branch && blank) return openTask(blank.id);
      setCreating(true);
      try {
        const id = await api.createTask(project, prompt, opts);
        await refresh();
        setView({ kind: "task", task: id });
      } catch (err) {
        fail(err);
      } finally {
        setCreating(false);
      }
    },
    [openTask, refresh, fail],
  );

  /** ⌘N：打开新建任务面板。 */
  const newTask = useCallback(
    (project?: string) => {
      if (!snapRef.current?.projects.length) return;
      setPanel({ project: project ?? currentProject() });
    },
    [currentProject],
  );

  const adder = useProjectAdder(refresh, fail);

  /**
   * 添加了项目（`known` 是添加之前已有的项目）：
   * 本来就有这个项目时打开它最近的任务；这是第一个项目时直接进它的命令行；否则在新建任务面板里选中它。
   */
  const projectAdded = useCallback(
    (id: string, known: string[]) => {
      const tasks = snapRef.current?.projects.find((p) => p.id === id)?.tasks ?? [];
      const latest = [...tasks].sort((a, b) => b.createdAt - a.createdAt)[0];
      if (known.includes(id) && latest) openTask(latest.id);
      else if (known.length === 0 || known.includes(id)) void createTask(id, "");
      else setPanel({ project: id });
    },
    [createTask, openTask],
  );
  const knownProjects = () => snapRef.current?.projects.map((p) => p.id) ?? [];

  /** 添加 `path` 指向的文件夹为项目（`duolo <目录>`、拖进窗口）。 */
  const addProject = useCallback(
    async (path: string) => {
      const known = knownProjects();
      const id = await adder.add(path);
      if (id) projectAdded(id, known);
    },
    [adder, projectAdded],
  );
  /** "添加项目"弹窗（侧边栏顶部的按钮、⌘O）。打开时记下已有的项目，添加后据此决定下一步。 */
  const [addingProject, setAddingProject] = useState<string[] | null>(null);
  const openAddProject = useCallback(() => setAddingProject(knownProjects()), []);

  // ---- 启动 ----

  useEffect(() => {
    const unlisten = listen<Snapshot>("snapshot", (e) => setSnap(e.payload));
    void api.startupNotices().then((list) => list.length > 0 && setNotices(list), () => {});
    void api.environment().then(setEnv, () => setEnv([]));
    void api.agentDefaults().then(setAgentDefaults, () => {});
    return () => void unlisten.then((f) => f());
  }, []);

  /** 重新检查环境，例如用户刚装好 Claude 或 Codex。 */
  const refreshEnv = useCallback(() => void api.environment().then(setEnv, () => {}), []);

  // 打开应用直接进入命令行：有任务就打开最近的那个，没有就新建一个。
  const booted = useRef(false);
  useEffect(() => {
    if (booted.current) return;
    booted.current = true;
    void (async () => {
      const s = await refresh();
      const requested = await api.takeOpenRequest();
      if (requested) return addProject(requested);
      const latest = allTasks(s).sort((a, b) => b.createdAt - a.createdAt)[0];
      if (latest) openTask(latest.id);
      else if (s.projects.length > 0) await createTask(s.projects[0].id, "");
    })();
  }, [refresh, addProject, openTask, createTask]);

  // 当前任务被关闭后，打开同项目里最近的任务。
  useEffect(() => {
    if (!snap || view.kind !== "task" || current) return;
    const next = allTasks(snap).sort((a, b) => b.createdAt - a.createdAt)[0];
    if (next) openTask(next.id);
    else setView({ kind: "empty" });
  }, [snap, view, current, openTask]);

  // 新任务的第一次输入作为它的需求，补上标题和分支名。
  const nameIfUntitled = useCallback(
    (ref: AgentRef, text: string) => {
      const found = findTask(snapRef.current, ref.task);
      if (!found || !found.task.untitled) return;
      // 先补上标题；AI 起好分支名后再换掉自动起的分支名（起名要一两秒，不让标题等它）。
      void (async () => {
        await api.nameTask(ref.task, text);
        await refresh();
        const branch = await api.suggestBranch(text);
        if (branch) {
          await api.renameBranch(ref.task, branch);
          await refresh();
        }
      })().catch(fail);
    },
    [refresh, fail],
  );
  useEffect(() => {
    setInputListener((ref, data) => {
      const found = findTask(snapRef.current, ref.task);
      if (!found || !found.task.untitled || writerOf(found.task)?.id !== ref.agent) return;
      const line = firstPrompt.feed(ref.task, data);
      if (line) nameIfUntitled(ref, line);
    });
    return () => setInputListener(null);
  }, [nameIfUntitled]);

  // ---- 其他操作 ----

  /** ⌘⇧R：有结论或正在审阅时开关面板，否则开始审阅。 */
  const toggleReview = useCallback(
    (task: number) => {
      const found = findTask(snapRef.current, task);
      if (!found) return;
      if (found.task.review || found.task.reviewingSince != null) {
        setReviewOpen((open) => (open === task ? null : task));
        return;
      }
      setReviewOpen(task);
      api.startReview(task).catch(fail);
    },
    [fail],
  );

  const requestClose = useCallback(
    (task: number) => {
      const found = findTask(snapRef.current, task);
      if (!found) return;
      const close = async () => {
        // 正在关闭的任务会先从列表里消失；没关成时再打开它，让用户看到原因和任务都在。
        const { savedTo, leftAt } = await api.closeTask(task).catch(async (err) => {
          await refresh();
          openTask(task);
          throw err;
        });
        disposeTask(task);
        firstPrompt.forget(task);
        if (savedTo) flash(t("task.savedTo", { branch: savedTo }));
        else if (leftAt) flash(t("task.leftAt", { path: leftAt }));
        await refresh();
      };
      // 还没输入过的新任务直接关掉，不用确认。
      if (found.task.untitled) return void close().catch(fail);
      setConfirm({
        title: t("confirm.closeTitle", { title: found.task.title }),
        body: t("confirm.closeBody", { branch: found.task.branch }),
        action: t("confirm.closeAction"),
        danger: true,
        run: close,
      });
    },
    [refresh, openTask, fail, flash, t],
  );

  const requestQuit = useCallback(() => {
    // 告诉后端界面还在，不用弹系统的"没有响应"。
    void api.quitSeen();
    const live = allTasks(snapRef.current)
      .flatMap((t) => t.agents)
      .filter((a) => isLive(a.status)).length;
    if (live === 0) return void api.quit();
    setConfirm({
      title: t("confirm.quitTitle"),
      // macOS 上关窗口只是隐藏；Windows / Linux 上关窗口就是退出，不提示。
      body: t("confirm.quitBody", { count: live }) + (isMac ? "\n" + t("confirm.quitHideHint") : ""),
      action: t("confirm.quitAction"),
      danger: true,
      run: () => api.quit(),
    });
  }, [t]);

  const cycleTask = useCallback(
    (delta: number) => {
      const tasks = allTasks(snapRef.current);
      if (tasks.length === 0) return;
      const index = view.kind === "task" ? tasks.findIndex((t) => t.id === view.task) : -1;
      openTask(tasks[(index + delta + tasks.length) % tasks.length].id);
    },
    [view, openTask],
  );

  /** 跳到下一个等你确认的任务（⌥↓）。 */
  const jumpToWaiting = useCallback(() => {
    const waiting = allTasks(snapRef.current).filter((task) => writerOf(task)?.status === "needs_input");
    if (waiting.length === 0) return;
    const index = view.kind === "task" ? waiting.findIndex((task) => task.id === view.task) : -1;
    openTask(waiting[(index + 1) % waiting.length].id);
  }, [view, openTask]);

  // ---- 语音 ----

  /** 新建任务面板打开时，语音文字插到面板里。 */
  const panelTarget = useRef<VoiceTarget | null>(null);
  const setPanelTarget = useCallback((target: VoiceTarget | null) => (panelTarget.current = target), []);
  const currentRef = useRef(current);
  currentRef.current = current;
  const voice = useVoice({
    ready: env?.find((c) => c.kind === "voice")?.ok ?? false,
    target: () => {
      if (panelTarget.current) return panelTarget.current;
      const cur = currentRef.current;
      const writer = cur && writerOf(cur.task);
      return cur && writer ? { kind: "agent", ref: { task: cur.task.id, agent: writer.id } } : null;
    },
    context: { project: current?.project.name, task: current && !current.task.untitled ? current.task.title : undefined },
    // 语音文字填进了输入框但还没发送：记进首次输入，用户按 ↵ 时一起作为需求。
    onInserted: (ref, text) => void firstPrompt.feed(ref.task, text),
    onError: fail,
    onNoTarget: () => flash(t("voice.needTask")),
  });

  // ---- 菜单和快捷键 ----

  const handlers = useRef<Record<string, () => void>>({});
  handlers.current = {
    quit: requestQuit,
    "new-task": () => void newTask(),
    "add-project": openAddProject,
    review: () => view.kind === "task" && toggleReview(view.task),
    "prev-task": () => cycleTask(-1),
    "next-task": () => cycleTask(1),
    reveal: () => current && void api.reveal(current.task.worktree),
    "close-task": () => view.kind === "task" && requestClose(view.task),
    voice: voice.toggle,
  };
  allTasks(snap)
    .slice(0, 9)
    .forEach((t, i) => (handlers.current[`task-${i + 1}`] = () => openTask(t.id)));

  handlers.current["search"] = () => setFocusSearch((n) => n + 1);
  handlers.current["next-waiting"] = jumpToWaiting;

  useEffect(() => {
    const unlistenMenu = listen<string>("menu", (e) => handlers.current[e.payload]?.());
    // macOS 上菜单里的快捷键由原生菜单处理；Windows / Linux 没有原生菜单，全部在这里处理。
    const shortcuts = isMac ? EXTRA_SHORTCUTS : [...MENU_SHORTCUTS, ...EXTRA_SHORTCUTS];
    const onKey = (e: KeyboardEvent) => {
      const hit = shortcuts.find(([, keys]) => matches(e, keys));
      if (!hit) return;
      e.preventDefault();
      e.stopPropagation();
      handlers.current[hit[0]]?.();
    };
    window.addEventListener("keydown", onKey, true);
    return () => {
      void unlistenMenu.then((f) => f());
      window.removeEventListener("keydown", onKey, true);
    };
  }, []);

  // `duolo <目录>` 打开的项目，以及拖进窗口的文件夹：交给最新的 addProject 处理。
  const addProjectRef = useRef(addProject);
  addProjectRef.current = addProject;
  useEffect(() => {
    const unlisten = listen<string>("open-project", (e) => void addProjectRef.current(e.payload));
    const unlistenDrop = getCurrentWebview().onDragDropEvent(({ payload }) => {
      if (payload.type === "enter" || payload.type === "over") setDragging(true);
      else if (payload.type === "leave") setDragging(false);
      else if (payload.type === "drop") {
        setDragging(false);
        if (payload.paths[0]) void addProjectRef.current(payload.paths[0]);
      }
    });
    return () => {
      void unlisten.then((f) => f());
      void unlistenDrop.then((f) => f());
    };
  }, []);

  return (
    <div className="app">
      <Sidebar
        snap={snap}
        view={view}
        focusSearch={focusSearch}
        onNewTask={newTask}
        onOpenTask={openTask}
        onOpenReview={(task) => {
          openTask(task);
          setReviewOpen(task);
        }}
        onStartReview={(task) => {
          openTask(task);
          toggleReview(task);
        }}
        onCloseTask={requestClose}
        onAddProject={openAddProject}
        onJumpToWaiting={jumpToWaiting}
        onError={fail}
      />
      <main className="main">
        {view.kind === "task" && current ? (
          <TaskView
            task={current.task}
            voice={voice}
            reviewOpen={reviewOpen === current.task.id}
            reviewerInstalled={isInstalled(env, current.task.reviewer.kind)}
            agents={agentOptions(env)}
            onReviewOpenChange={(open) => setReviewOpen(open ? current.task.id : null)}
            onConfirm={setConfirm}
            onClose={() => requestClose(view.task)}
            onError={fail}
          />
        ) : (
          <Empty
            snap={snap}
            env={env}
            creating={creating}
            dragging={dragging}
            adder={adder}
            onNewTask={() => newTask()}
            onProjectAdded={(id) => projectAdded(id, [])}
          />
        )}
      </main>
      {panel && snap && (
        <NewTaskPanel
          snap={snap}
          project={panel.project}
          voice={voice}
          setVoiceTarget={setPanelTarget}
          env={env}
          defaults={agentDefaults}
          onRefreshEnv={refreshEnv}
          onCreate={createTask}
          onAddProject={openAddProject}
          onError={fail}
          onClose={() => setPanel(null)}
        />
      )}
      {addingProject && (
        <AddProjectDialog
          adder={adder}
          onAdded={(id) => {
            const known = addingProject;
            setAddingProject(null);
            projectAdded(id, known);
          }}
          onClose={() => setAddingProject(null)}
        />
      )}
      {confirm && <Confirm spec={confirm} onDone={() => setConfirm(null)} onError={fail} />}
      {notices.length > 0 && <Notice problems={notices} onClose={clearNotices} />}
      {toast && (
        <div className="toast" onClick={() => setToast(null)}>
          {toast}
        </div>
      )}
    </div>
  );
}
