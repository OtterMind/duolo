import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { open as openDialog } from "@tauri-apps/plugin-dialog";

import { api, errorText, type AgentRef, type Check, type Snapshot } from "./lib/api";
import { allTasks, findTask, isLive, reviewerOf, writerOf } from "./lib/format";
import { disposeTask } from "./lib/terminals";
import { Sidebar } from "./components/Sidebar";
import { Home } from "./components/Home";
import { TaskView } from "./components/TaskView";
import { Confirm, type ConfirmSpec } from "./components/Confirm";
import { useVoice } from "./lib/voice";

export type View = { kind: "home" } | { kind: "task"; task: number; agent: number };

export function App() {
  const [snap, setSnap] = useState<Snapshot | null>(null);
  const [env, setEnv] = useState<Check[] | null>(null);
  const [view, setView] = useState<View>({ kind: "home" });
  const [homeProject, setHomeProject] = useState<string | null>(null);
  const [homeText, setHomeText] = useState("");
  const [confirm, setConfirm] = useState<ConfirmSpec | null>(null);
  const [toast, setToast] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  /** 每个任务上次看的是哪个 Agent。 */
  const lastAgent = useRef(new Map<number, number>());

  const flash = useCallback((text: string) => {
    setToast(text);
    window.setTimeout(() => setToast((t) => (t === text ? null : t)), 4000);
  }, []);
  const fail = useCallback((err: unknown) => flash(errorText(err)), [flash]);

  // ---- 数据 ----

  const refresh = useCallback(async () => setSnap(await api.snapshot()), []);

  useEffect(() => {
    void refresh();
    void api.environment().then(setEnv).catch(() => setEnv([]));
    const unlisten = listen<Snapshot>("snapshot", (e) => setSnap(e.payload));
    return () => void unlisten.then((f) => f());
  }, [refresh]);

  // 首页默认选中第一个项目。
  useEffect(() => {
    if (!snap) return;
    if (!homeProject || !snap.projects.some((p) => p.id === homeProject)) {
      setHomeProject(snap.projects[0]?.id ?? null);
    }
  }, [snap, homeProject]);

  // 当前任务被关闭后回到首页。
  const current = view.kind === "task" ? findTask(snap, view.task) : null;
  useEffect(() => {
    if (snap && view.kind === "task" && !current) setView({ kind: "home" });
  }, [snap, view, current]);

  // ---- 操作 ----

  const openTask = useCallback(
    (task: number, agent?: number) => {
      const found = findTask(snap, task);
      if (!found) return;
      const pick =
        agent ??
        lastAgent.current.get(task) ??
        found.task.agents.find((a) => a.status === "needs_input")?.id ??
        writerOf(found.task)?.id ??
        0;
      lastAgent.current.set(task, pick);
      setView({ kind: "task", task, agent: pick });
    },
    [snap],
  );

  const goHome = useCallback(
    (project?: string) => {
      if (project) setHomeProject(project);
      else if (current) setHomeProject(current.project.id);
      setView({ kind: "home" });
    },
    [current],
  );

  const addProject = useCallback(
    async (path?: string) => {
      const chosen = path ?? (await openDialog({ directory: true, title: "选择一个 git 仓库" }));
      if (typeof chosen !== "string") return null;
      try {
        const id = await api.addProject(chosen);
        await refresh();
        setHomeProject(id);
        setView({ kind: "home" });
        return id;
      } catch (err) {
        fail(err);
        return null;
      }
    },
    [refresh, fail],
  );

  const createTask = useCallback(
    async (prompt: string) => {
      let project = homeProject;
      if (!project) project = await addProject();
      if (!project) return;
      setBusy(true);
      try {
        const id = await api.createTask(project, prompt);
        setHomeText("");
        const fresh = await api.snapshot();
        setSnap(fresh);
        lastAgent.current.set(id, 0);
        setView({ kind: "task", task: id, agent: 0 });
      } catch (err) {
        fail(err);
      } finally {
        setBusy(false);
      }
    },
    [homeProject, addProject, fail],
  );

  const startReview = useCallback(
    async (task: number) => {
      try {
        const agent = await api.startReviewer(task);
        await refresh();
        lastAgent.current.set(task, agent);
        setView({ kind: "task", task, agent });
      } catch (err) {
        fail(err);
      }
    },
    [refresh, fail],
  );

  const requestClose = useCallback(
    (task: number) => {
      const found = findTask(snap, task);
      if (!found) return;
      setConfirm({
        title: `关闭「${found.task.title}」？`,
        body: `会结束这个任务的 Agent 并删除工作目录。分支 ${found.task.branch} 会保留，改动不会丢。`,
        action: "关闭任务",
        danger: true,
        run: async () => {
          await api.closeTask(task);
          disposeTask(task);
          lastAgent.current.delete(task);
          await refresh();
        },
      });
    },
    [snap, refresh],
  );

  const requestQuit = useCallback(() => {
    const live = allTasks(snap).flatMap((t) => t.agents).filter((a) => isLive(a.status)).length;
    if (live === 0) return void api.quit();
    setConfirm({
      title: "退出 Duolo？",
      body: `还有 ${live} 个 Agent 在运行，退出会结束它们。下次打开时可以继续之前的会话。\n只想收起窗口的话，用 ⌘W。`,
      action: "退出",
      danger: true,
      run: () => api.quit(),
    });
  }, [snap]);

  const cycleTask = useCallback(
    (delta: number) => {
      const tasks = allTasks(snap);
      if (tasks.length === 0) return;
      const index = view.kind === "task" ? tasks.findIndex((t) => t.id === view.task) : -1;
      const next = tasks[(index + delta + tasks.length) % tasks.length];
      openTask(next.id);
    },
    [snap, view, openTask],
  );

  const cycleAgent = useCallback(() => {
    if (view.kind !== "task" || !current) return;
    const agents = current.task.agents;
    const i = agents.findIndex((a) => a.id === view.agent);
    openTask(view.task, agents[(i + 1) % agents.length].id);
  }, [view, current, openTask]);

  // ---- 语音 ----

  const voiceReady = env?.find((c) => c.name === "语音")?.ok ?? false;
  const voiceTarget: AgentRef | null = view.kind === "task" ? { task: view.task, agent: view.agent } : null;
  const voice = useVoice({
    ready: voiceReady,
    target: voiceTarget,
    context: { project: current?.project.name ?? snap?.projects.find((p) => p.id === homeProject)?.name, task: current?.task.title },
    onHomeText: (text) => setHomeText((t) => (t.trim() ? `${t.trimEnd()} ${text}` : text)),
    onError: fail,
  });

  // ---- 菜单和快捷键 ----

  const handlers = useRef<Record<string, () => void>>({});
  handlers.current = {
    quit: requestQuit,
    "new-task": () => goHome(),
    "add-project": () => void addProject(),
    review: () => view.kind === "task" && void startReview(view.task),
    "next-agent": cycleAgent,
    "prev-task": () => cycleTask(-1),
    "next-task": () => cycleTask(1),
    reveal: () => current && void api.reveal(current.task.worktree),
    "close-task": () => view.kind === "task" && requestClose(view.task),
    voice: voice.toggle,
  };

  useEffect(() => {
    const unlistenMenu = listen<string>("menu", (e) => handlers.current[e.payload]?.());
    const unlistenOpen = listen<string>("open-project", (e) => void addProject(e.payload));
    void api.takeOpenRequest().then((path) => path && addProject(path));
    // ⌘1…⌘9 跳到第 n 个任务。菜单里不列这些，直接在这里处理。
    const onKey = (e: KeyboardEvent) => {
      if (e.metaKey && !e.shiftKey && !e.altKey && /^[1-9]$/.test(e.key)) {
        e.preventDefault();
        handlers.current[`task-${e.key}`]?.();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => {
      void unlistenMenu.then((f) => f());
      void unlistenOpen.then((f) => f());
      window.removeEventListener("keydown", onKey);
    };
    // 只在首次挂载时注册，回调通过 handlers 读取最新状态。
  }, []);
  allTasks(snap)
    .slice(0, 9)
    .forEach((t, i) => (handlers.current[`task-${i + 1}`] = () => openTask(t.id)));

  return (
    <div className="app">
      <Sidebar
        snap={snap}
        view={view}
        onNewTask={goHome}
        onOpenTask={openTask}
        onCloseTask={requestClose}
        onAddProject={() => void addProject()}
      />
      <main className="main">
        {view.kind === "task" && current ? (
          <TaskView
            key={current.task.id}
            task={current.task}
            agentId={view.agent}
            voice={voice}
            onSelectAgent={(agent) => openTask(view.task, agent)}
            onReview={() => void startReview(view.task)}
            onClose={() => requestClose(view.task)}
            hasReviewer={!!reviewerOf(current.task)}
            onError={fail}
          />
        ) : (
          <Home
            snap={snap}
            env={env}
            project={homeProject}
            onProjectChange={setHomeProject}
            onAddProject={() => void addProject()}
            text={homeText}
            onTextChange={setHomeText}
            onSubmit={createTask}
            busy={busy}
            voice={voice}
          />
        )}
      </main>
      {confirm && <Confirm spec={confirm} onDone={() => setConfirm(null)} onError={fail} />}
      {toast && (
        <div className="toast" onClick={() => setToast(null)}>
          {toast}
        </div>
      )}
    </div>
  );
}
