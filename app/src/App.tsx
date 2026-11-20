import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { open as openDialog } from "@tauri-apps/plugin-dialog";

import { api, errorText, type AgentRef, type Check, type Snapshot } from "./lib/api";
import { allTasks, findTask, isLive, writerOf } from "./lib/format";
import * as firstPrompt from "./lib/firstPrompt";
import { disposeTask, setInputListener } from "./lib/terminals";
import { useVoice } from "./lib/voice";
import { Sidebar } from "./components/Sidebar";
import { Empty } from "./components/Empty";
import { TaskView } from "./components/TaskView";
import { Confirm, type ConfirmSpec } from "./components/Confirm";

/** 右侧显示什么：某个任务的某个 Agent，或者没有任务时的空状态。 */
export type View = { kind: "empty" } | { kind: "task"; task: number; agent: number };

export function App() {
  const [snap, setSnap] = useState<Snapshot | null>(null);
  const [env, setEnv] = useState<Check[] | null>(null);
  const [view, setView] = useState<View>({ kind: "empty" });
  const [creating, setCreating] = useState(false);
  const [confirm, setConfirm] = useState<ConfirmSpec | null>(null);
  const [toast, setToast] = useState<string | null>(null);
  /** 每个任务上次看的是哪个 Agent。 */
  const lastAgent = useRef(new Map<number, number>());
  const snapRef = useRef(snap);
  snapRef.current = snap;

  const flash = useCallback((text: string) => {
    setToast(text);
    window.setTimeout(() => setToast((t) => (t === text ? null : t)), 4000);
  }, []);
  const fail = useCallback((err: unknown) => flash(errorText(err)), [flash]);

  const refresh = useCallback(async () => {
    const fresh = await api.snapshot();
    setSnap(fresh);
    return fresh;
  }, []);

  const current = view.kind === "task" ? findTask(snap, view.task) : null;

  // ---- 打开和新建任务 ----

  const openTask = useCallback((task: number, agent?: number) => {
    const found = findTask(snapRef.current, task);
    if (!found) return;
    const pick =
      agent ??
      lastAgent.current.get(task) ??
      found.task.agents.find((a) => a.status === "needs_input")?.id ??
      writerOf(found.task)?.id ??
      0;
    lastAgent.current.set(task, pick);
    setView({ kind: "task", task, agent: pick });
  }, []);

  /** 新任务：直接打开 Agent 的命令行。项目里已有还没输入过的新任务时复用它。 */
  const newTask = useCallback(
    async (project?: string) => {
      const s = snapRef.current;
      const target =
        project ??
        (view.kind === "task" ? findTask(s, view.task)?.project.id : undefined) ??
        s?.projects[0]?.id;
      if (!target) return;
      const blank = s?.projects.find((p) => p.id === target)?.tasks.find((t) => t.prompt === "");
      if (blank) return openTask(blank.id, writerOf(blank)?.id);
      setCreating(true);
      try {
        const id = await api.createTask(target, "");
        await refresh();
        lastAgent.current.set(id, 0);
        setView({ kind: "task", task: id, agent: 0 });
      } catch (err) {
        fail(err);
      } finally {
        setCreating(false);
      }
    },
    [view, openTask, refresh, fail],
  );

  const addProject = useCallback(
    async (path?: string) => {
      const chosen = path ?? (await openDialog({ directory: true, title: "选择一个 git 仓库" }));
      if (typeof chosen !== "string") return;
      try {
        const id = await api.addProject(chosen);
        await refresh();
        await newTask(id);
      } catch (err) {
        fail(err);
      }
    },
    [refresh, newTask, fail],
  );

  // ---- 启动 ----

  useEffect(() => {
    const unlisten = listen<Snapshot>("snapshot", (e) => setSnap(e.payload));
    void api.environment().then(setEnv, () => setEnv([]));
    return () => void unlisten.then((f) => f());
  }, []);

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
      else if (s.projects.length > 0) await newTask(s.projects[0].id);
    })();
  }, [refresh, addProject, openTask, newTask]);

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
      if (!found || found.task.prompt !== "") return;
      void api.nameTask(ref.task, text).then(refresh, fail);
    },
    [refresh, fail],
  );
  useEffect(() => {
    setInputListener((ref, data) => {
      const found = findTask(snapRef.current, ref.task);
      if (!found || found.task.prompt !== "" || writerOf(found.task)?.id !== ref.agent) return;
      const line = firstPrompt.feed(ref.task, data);
      if (line) nameIfUntitled(ref, line);
    });
    return () => setInputListener(null);
  }, [nameIfUntitled]);

  // ---- 其他操作 ----

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
      const found = findTask(snapRef.current, task);
      if (!found) return;
      const close = async () => {
        await api.closeTask(task);
        disposeTask(task);
        firstPrompt.forget(task);
        lastAgent.current.delete(task);
        await refresh();
      };
      // 还没输入过的新任务直接关掉，不用确认。
      if (found.task.prompt === "") return void close().catch(fail);
      setConfirm({
        title: `关闭「${found.task.title}」？`,
        body: `会结束这个任务的 Agent 并删除工作目录。分支 ${found.task.branch} 会保留，改动不会丢。`,
        action: "关闭任务",
        danger: true,
        run: close,
      });
    },
    [refresh, fail],
  );

  const requestQuit = useCallback(() => {
    const live = allTasks(snapRef.current)
      .flatMap((t) => t.agents)
      .filter((a) => isLive(a.status)).length;
    if (live === 0) return void api.quit();
    setConfirm({
      title: "退出 Duolo？",
      body: `还有 ${live} 个 Agent 在运行，退出会结束它们。下次打开时可以继续之前的会话。\n只想收起窗口的话，用 ⌘W。`,
      action: "退出",
      danger: true,
      run: () => api.quit(),
    });
  }, []);

  const cycleTask = useCallback(
    (delta: number) => {
      const tasks = allTasks(snapRef.current);
      if (tasks.length === 0) return;
      const index = view.kind === "task" ? tasks.findIndex((t) => t.id === view.task) : -1;
      openTask(tasks[(index + delta + tasks.length) % tasks.length].id);
    },
    [view, openTask],
  );

  const cycleAgent = useCallback(() => {
    if (view.kind !== "task" || !current) return;
    const agents = current.task.agents;
    const i = agents.findIndex((a) => a.id === view.agent);
    openTask(view.task, agents[(i + 1) % agents.length].id);
  }, [view, current, openTask]);

  // ---- 语音 ----

  const voice = useVoice({
    ready: env?.find((c) => c.name === "语音")?.ok ?? false,
    target: view.kind === "task" ? { task: view.task, agent: view.agent } : null,
    context: { project: current?.project.name, task: current?.task.prompt ? current.task.title : undefined },
    onSent: nameIfUntitled,
    onError: fail,
  });

  // ---- 菜单和快捷键 ----

  const handlers = useRef<Record<string, () => void>>({});
  handlers.current = {
    quit: requestQuit,
    "new-task": () => void newTask(),
    "add-project": () => void addProject(),
    review: () => view.kind === "task" && void startReview(view.task),
    "next-agent": cycleAgent,
    "prev-task": () => cycleTask(-1),
    "next-task": () => cycleTask(1),
    reveal: () => current && void api.reveal(current.task.worktree),
    "close-task": () => view.kind === "task" && requestClose(view.task),
    voice: voice.toggle,
  };
  allTasks(snap)
    .slice(0, 9)
    .forEach((t, i) => (handlers.current[`task-${i + 1}`] = () => openTask(t.id)));

  useEffect(() => {
    const unlistenMenu = listen<string>("menu", (e) => handlers.current[e.payload]?.());
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
      window.removeEventListener("keydown", onKey);
    };
  }, []);

  // `duolo <目录>` 打开的项目：事件里带的是路径，交给最新的 addProject 处理。
  useEffect(() => {
    const unlisten = listen<string>("open-project", (e) => void addProject(e.payload));
    return () => void unlisten.then((f) => f());
  }, [addProject]);

  return (
    <div className="app">
      <Sidebar
        snap={snap}
        view={view}
        onNewTask={(project) => void newTask(project)}
        onOpenTask={openTask}
        onCloseTask={requestClose}
        onAddProject={() => void addProject()}
      />
      <main className="main">
        {view.kind === "task" && current ? (
          <TaskView
            task={current.task}
            agentId={view.agent}
            voice={voice}
            onSelectAgent={(agent) => openTask(view.task, agent)}
            onReview={() => void startReview(view.task)}
            onClose={() => requestClose(view.task)}
            onError={fail}
          />
        ) : (
          <Empty
            snap={snap}
            env={env}
            creating={creating}
            onNewTask={() => void newTask()}
            onAddProject={() => void addProject()}
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
