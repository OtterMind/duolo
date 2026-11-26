import { useEffect, useLayoutEffect, useRef, useState } from "react";
import {
  ArrowRight,
  ArrowUp,
  Check,
  Ellipsis,
  Folder,
  FolderGit2,
  FolderPlus,
  GitBranch,
  MessagesSquare,
  Mic,
  Paperclip,
  Pencil,
  Plus,
  Search,
  Sparkles,
  SquareTerminal,
  TriangleAlert,
  X,
} from "lucide-react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";

import { useT } from "../i18n";
import { api, type Check as EnvCheck, type NewTaskOptions, type Repo, type Snapshot, type Surface } from "../lib/api";
import {
  agentOptions,
  canSetUp,
  defaultChoice,
  noAgentInstalled,
  rememberChoice,
  type AgentChoice,
  type AgentOption,
} from "../lib/agents";
import type { Voice, VoiceTarget } from "../lib/voice";
import { useModalFocus } from "../lib/modal";
import { pluginName, pluginOf } from "../lib/plugins";
import { AgentIcon } from "./AgentIcon";
import { AgentMenu, CopyCommand, Popover } from "./AgentMenu";
import { onSetupReady, requestSetup } from "../lib/setup";
import { Waveform } from "./VoiceBar";
import { TALK_KEY } from "../lib/platform";

interface Props {
  snap: Snapshot;
  project: string | null;
  voice: Voice;
  /** 面板打开期间，语音文字插到面板的输入框里。 */
  setVoiceTarget(target: VoiceTarget | null): void;
  /** 环境检查结果：哪些 Agent 装了。 */
  env: EnvCheck[] | null;
  /** 配置里默认的写代码和审阅 Agent。 */
  defaults: AgentChoice | null;
  /** 重新检查环境（用户可能刚装好 Agent）。 */
  onRefreshEnv(): void;
  onCreate(project: string, prompt: string, opts: NewTaskOptions): Promise<void>;
  /** 打开"添加项目"弹窗。 */
  onAddProject(): void;
  onError(err: unknown): void;
  onClose(): void;
}

const ICON = { size: 14, strokeWidth: 1.75 } as const;
/** 停止输入多久后让 AI 起分支名。 */
const NAMING_DELAY_MS = 900;
const LAST_REPO_KEY = "duolo.lastRepo";

type Menu = "project" | "repo" | "base" | "writer" | null;

/**
 * 新建任务（项目后面的 +，或 ⌘N）。写一句需求、回车就开始，其他都有默认值：
 * 第一行是"在哪里改"——项目 / 仓库 / 基于哪个分支 → 新分支名（AI 根据需求起名，可以改）；
 * 第二行是谁写谁审。什么都不写直接回车，就开一个空白任务。
 */
export function NewTaskPanel(props: Props) {
  const { snap, voice, setVoiceTarget, env, defaults, onCreate, onClose } = props;
  const t = useT();
  const [project, setProject] = useState(props.project ?? snap.projects[0]?.id ?? null);
  const [prompt, setPrompt] = useState("");
  const [creating, setCreating] = useState(false);
  const [open, setOpen] = useState<Menu>(null);
  const input = useRef<HTMLTextAreaElement>(null);
  const promptRef = useRef(prompt);
  promptRef.current = prompt;
  const current = snap.projects.find((p) => p.id === project);

  const { repos, repo, setRepo, reload: reloadRepos } = useRepos(project, props.onError);
  const [base, setBase] = useState<string | null>(null);
  useEffect(() => setBase(null), [repo]);
  const branch = useBranchName(prompt);

  const [choice, setChoice] = useState<AgentChoice>(() => defaultChoice(env, defaults));
  // 环境检查晚于面板打开时，按检查结果重新挑一次默认值（之前挑的可能没装）。
  const pickedAgents = useRef(false);
  useEffect(() => {
    if (!pickedAgents.current) setChoice(defaultChoice(env, defaults));
  }, [env, defaults]);
  useEffect(() => props.onRefreshEnv(), []);
  const agents = agentOptions(env);
  const noAgent = noAgentInstalled(env);
  const pickWriter = (id: string) => {
    pickedAgents.current = true;
    setChoice((c) => ({ ...c, writer: id }));
  };
  // 一键装好、登录好的 Agent 顺手选上。
  useEffect(() => onSetupReady(pickWriter), []);
  /** 在输入框里按 ⇥ / ⇧⇥ 轮换写代码的 Agent（只在装了的里面换）。 */
  const cycleWriter = (delta: number) => {
    const installed = agents.filter((a) => a.installed);
    if (installed.length < 2) return;
    const at = installed.findIndex((a) => a.id === choice.writer);
    pickWriter(installed[(at + delta + installed.length) % installed.length].id);
  };

  // 在终端里还是聊天模式下运行：默认和上次一样（任务里切换过也算）。
  const [surface, setSurface] = useState<Surface>("terminal");
  const pickedSurface = useRef(false);
  useEffect(() => {
    void api.agentDefaults().then((d) => pickedSurface.current || setSurface(d.surface), () => {});
  }, []);
  const chatCapable = pluginOf(choice.writer)?.chat === true;

  useModalFocus(input);
  const refocus = () => {
    setOpen(null);
    input.current?.focus();
  };

  // 语音文字插到光标处。
  useEffect(() => {
    setVoiceTarget({
      kind: "text",
      insert: (text) => {
        const el = input.current;
        const at = el?.selectionStart ?? promptRef.current.length;
        const before = promptRef.current.slice(0, at);
        const glue = before && !/\s$/.test(before) ? " " : "";
        setPrompt(before + glue + text + promptRef.current.slice(at));
        el?.focus();
      },
      replace: (old, next) => {
        const i = promptRef.current.lastIndexOf(old);
        if (i >= 0) setPrompt(promptRef.current.slice(0, i) + next + promptRef.current.slice(i + old.length));
      },
    });
    return () => setVoiceTarget(null);
  }, [setVoiceTarget]);

  // 输入框随内容长高。
  useLayoutEffect(() => {
    const el = input.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${Math.min(el.scrollHeight, 220)}px`;
  }, [prompt]);

  // 选「终端」不需要装任何 Agent。
  const shell = choice.writer === "shell";
  const canStart = !!project && !!repo && (!noAgent || shell) && !creating;
  const submit = async () => {
    if (!canStart || !project) return;
    setCreating(true);
    try {
      rememberChoice(choice);
      // 没动过就交给后端按上次的定；动过的记下来，下次照着用。
      const chosen = pickedSurface.current && chatCapable ? surface : undefined;
      await onCreate(project, prompt.trim(), { repo, base, branch: branch.value, ...choice, surface: chosen });
    } finally {
      setCreating(false);
    }
  };

  const attach = async () => {
    const files = await openDialog({ multiple: true, defaultPath: repo ?? current?.root });
    const list = Array.isArray(files) ? files : typeof files === "string" ? [files] : [];
    if (list.length === 0) return;
    const refs = list.map((f) => `@${f}`).join(" ");
    setPrompt((p) => (p && !/\s$/.test(p) ? `${p} ${refs} ` : `${p}${refs} `));
    input.current?.focus();
  };

  const listening = voice.targetKind === "text" && voice.phase.kind !== "idle";
  const repoLabel = (r: Repo) => r.name || current?.name || "";
  const currentRepo = repos?.find((r) => r.path === repo);

  return (
    <div
      className="scrim panel-scrim"
      onMouseDown={(e) => e.target === e.currentTarget && onClose()}
      onKeyDown={(e) => {
        if (e.key !== "Escape") return;
        if (open) setOpen(null);
        else if (!branch.editing && voice.phase.kind === "idle") onClose();
      }}
    >
      <div className="new-task" role="dialog" aria-label={t("sidebar.newTask")}>
        <textarea
          ref={input}
          className="new-task-input"
          rows={2}
          value={prompt}
          placeholder={shell ? t("newTask.shellPlaceholder") : t("newTask.placeholder", { agent: pluginName(choice.writer) })}
          onChange={(e) => setPrompt(e.target.value)}
          onKeyDown={(e) => {
            if (e.nativeEvent.isComposing) return;
            if (e.key === "Enter" && !e.shiftKey) {
              e.preventDefault();
              void submit();
            } else if (e.key === "Tab" && !e.altKey && !e.metaKey && !e.ctrlKey) {
              e.preventDefault();
              cycleWriter(e.shiftKey ? -1 : 1);
            }
          }}
        />
        {listening && (
          <div className="new-task-voice">
            {voice.phase.kind === "recording" ? (
              <>
                <span className="rec-dot" />
                <Waveform levels={voice.levels} silent={voice.notice === "silent"} />
                <span className="grow ellipsis">{voice.notice === "silent" ? t("voice.silent") : voice.heard}</span>
              </>
            ) : (
              <>
                <span className="status status-running" />
                <span className="grow">{t("voice.processing")}</span>
              </>
            )}
          </div>
        )}

        {/* 在哪里改：项目 / 仓库 / 基于哪个分支 → 新分支 */}
        <div className="new-task-where">
          <div className="dropdown-anchor">
            <button
              className={`chip ${open === "project" ? "is-open" : ""}`}
              title={current ? `${t("newTask.project")} · ${current.root}` : undefined}
              onClick={() => setOpen(open === "project" ? null : "project")}
            >
              <Folder size={13} strokeWidth={1.75} />
              <span className="ellipsis">{current?.name ?? t("newTask.chooseProject")}</span>
            </button>
            {open === "project" && (
              <ProjectMenu
                snap={snap}
                selected={project}
                onPick={(id) => {
                  setProject(id);
                  refocus();
                }}
                onAddProject={() => {
                  setOpen(null);
                  props.onAddProject();
                }}
                onClose={() => setOpen(null)}
              />
            )}
          </div>

          {project && repos && repos.length !== 1 && (
            <div className="dropdown-anchor">
              <button
                className={`chip ${repos.length === 0 ? "is-warn" : ""} ${open === "repo" ? "is-open" : ""}`}
                title={t("newTask.repo")}
                onClick={() => setOpen(open === "repo" ? null : "repo")}
              >
                {repos.length === 0 ? (
                  <TriangleAlert size={13} strokeWidth={1.75} />
                ) : (
                  <FolderGit2 size={13} strokeWidth={1.75} />
                )}
                <span className="ellipsis">{currentRepo ? repoLabel(currentRepo) : t("newTask.noRepo")}</span>
              </button>
              {open === "repo" && (
                <RepoMenu
                  project={project}
                  repos={repos}
                  selected={repo}
                  label={repoLabel}
                  onPick={(path) => {
                    setRepo(path);
                    refocus();
                  }}
                  onCreated={(path) => {
                    void reloadRepos().then(() => setRepo(path));
                    refocus();
                  }}
                  onError={props.onError}
                  onClose={() => setOpen(null)}
                />
              )}
            </div>
          )}

          {repo && (
            <>
              <div className="dropdown-anchor">
                <button
                  className={`chip mono ${open === "base" ? "is-open" : ""}`}
                  title={t("newTask.baseBranch")}
                  onClick={() => setOpen(open === "base" ? null : "base")}
                >
                  <GitBranch size={12} strokeWidth={1.75} />
                  <BranchName repo={repo} base={base} />
                </button>
                {open === "base" && (
                  <BranchMenu
                    repo={repo}
                    selected={base}
                    onPick={(b) => {
                      setBase(b);
                      refocus();
                    }}
                    onClose={() => setOpen(null)}
                  />
                )}
              </div>
              <ArrowRight size={12} strokeWidth={1.75} className="dim" />
              <BranchField branch={branch} />
            </>
          )}
        </div>

        {/* 谁来写，以及附件、语音、开始 */}
        <div className="new-task-bar">
          <AgentPicker
            agents={agents}
            selected={choice.writer}
            menuOpen={open === "writer"}
            onMenu={(show) => setOpen(show ? "writer" : null)}
            onPick={(id) => {
              pickWriter(id);
              refocus();
            }}
          />
          {chatCapable && (
            <div className="segmented" role="group">
              <button
                className={`icon-btn ${surface === "terminal" ? "is-on" : ""}`}
                title={t("newTask.surfaceTerminal", { agent: pluginName(choice.writer) })}
                onClick={() => {
                  pickedSurface.current = true;
                  setSurface("terminal");
                  refocus();
                }}
              >
                <SquareTerminal {...ICON} />
              </button>
              <button
                className={`icon-btn ${surface === "chat" ? "is-on" : ""}`}
                title={t("newTask.surfaceChat", { agent: pluginName(choice.writer) })}
                onClick={() => {
                  pickedSurface.current = true;
                  setSurface("chat");
                  refocus();
                }}
              >
                <MessagesSquare {...ICON} />
              </button>
            </div>
          )}
          <span className="grow" />
          <button className="icon-btn" title={t("newTask.attach")} onClick={() => void attach()}>
            <Paperclip {...ICON} />
          </button>
          <button
            className={`icon-btn ${listening ? "is-recording" : ""}`}
            title={t("voice.title", { key: TALK_KEY })}
            onClick={voice.toggle}
          >
            <Mic {...ICON} />
          </button>
          <button
            className="send-btn"
            title={prompt.trim() ? t("newTask.start") : t("newTask.startBlank")}
            disabled={!canStart}
            onClick={() => void submit()}
          >
            {creating ? <span className="status status-running" /> : <ArrowUp size={15} strokeWidth={2.2} />}
          </button>
        </div>
      </div>
      {noAgent ? (
        <NoAgentNotice agents={agents} onRecheck={props.onRefreshEnv} />
      ) : (
        <SetupHint agents={agents} writer={choice.writer} />
      )}
    </div>
  );
}

/** 项目里的仓库，记住每个项目上次选的那个。 */
function useRepos(project: string | null, onError: (err: unknown) => void) {
  const [repos, setRepos] = useState<Repo[] | null>(null);
  const [repo, setRepoState] = useState<string | null>(null);
  const load = async () => {
    if (!project) return;
    try {
      const list = await api.projectRepos(project);
      setRepos(list);
      const last = readLastRepo(project);
      setRepoState(list.find((r) => r.path === last)?.path ?? list[0]?.path ?? null);
    } catch (err) {
      onError(err);
      setRepos([]);
    }
  };
  useEffect(() => {
    setRepos(null);
    setRepoState(null);
    void load();
  }, [project]);
  const setRepo = (path: string) => {
    setRepoState(path);
    if (project) writeLastRepo(project, path);
  };
  return { repos, repo, setRepo, reload: load };
}

function readLastRepo(project: string): string | null {
  try {
    return (JSON.parse(localStorage.getItem(LAST_REPO_KEY) ?? "{}") as Record<string, string>)[project] ?? null;
  } catch {
    return null;
  }
}

function writeLastRepo(project: string, path: string) {
  try {
    const all = JSON.parse(localStorage.getItem(LAST_REPO_KEY) ?? "{}") as Record<string, string>;
    localStorage.setItem(LAST_REPO_KEY, JSON.stringify({ ...all, [project]: path }));
  } catch {
    // 记不住就下次用第一个仓库。
  }
}

interface BranchNameState {
  /** 新建时用的分支名（AI 起的或用户改的）；null 表示用默认名字。 */
  value: string | null;
  naming: boolean;
  edited: boolean;
  editing: boolean;
  setEditing(editing: boolean): void;
  /** 用户自己定名字；空字符串表示交回给 AI。 */
  set(name: string): void;
}

/** 新分支名：用户停止输入一会儿后让 AI 根据需求起名；用户自己改过就不再自动起。 */
function useBranchName(prompt: string): BranchNameState {
  const [suggested, setSuggested] = useState<string | null>(null);
  const [edited, setEdited] = useState<string | null>(null);
  const [naming, setNaming] = useState(false);
  const [editing, setEditing] = useState(false);
  const request = useRef(0);

  useEffect(() => {
    const text = prompt.trim();
    if (!text) setSuggested(null);
    if (edited !== null || text.length < 4) return;
    const id = ++request.current;
    const timer = window.setTimeout(() => {
      setNaming(true);
      api
        .suggestBranch(text)
        .then((name) => id === request.current && name && setSuggested(name))
        .catch(() => {})
        .finally(() => id === request.current && setNaming(false));
    }, NAMING_DELAY_MS);
    return () => window.clearTimeout(timer);
  }, [prompt, edited]);

  return {
    value: edited ?? suggested,
    naming: naming && edited === null,
    edited: edited !== null,
    editing,
    setEditing,
    set: (name) => setEdited(name.trim() ? name.trim() : null),
  };
}

/** 新分支名：显示 AI 起的名字（起名中有个小动画），点一下可以自己改。 */
function BranchField({ branch }: { branch: BranchNameState }) {
  const t = useT();
  const [draft, setDraft] = useState("");
  if (branch.editing) {
    const finish = (save: boolean) => {
      if (save) branch.set(draft);
      branch.setEditing(false);
    };
    return (
      <input
        autoFocus
        className="branch-input"
        value={draft}
        placeholder={t("newTask.branchPlaceholder")}
        aria-label={t("newTask.newBranch")}
        onChange={(e) => setDraft(e.target.value)}
        onBlur={() => finish(true)}
        onKeyDown={(e) => {
          e.stopPropagation();
          if (e.key === "Enter") finish(true);
          if (e.key === "Escape") finish(false);
        }}
      />
    );
  }
  return (
    <button
      className={`chip mono branch-chip ${branch.value ? "" : "is-auto"}`}
      title={t("newTask.newBranchTitle")}
      onClick={() => {
        setDraft(branch.value ?? "");
        branch.setEditing(true);
      }}
    >
      {branch.naming ? (
        <span className="status status-running" />
      ) : (
        !branch.edited && <Sparkles size={12} strokeWidth={1.75} />
      )}
      <span className="ellipsis">{branch.value ?? t("newTask.branchAuto")}</span>
      <Pencil size={11} strokeWidth={1.75} className="dim" />
    </button>
  );
}

/** 选项目：已添加的项目，最下面是"添加项目…"。 */
function ProjectMenu({
  snap,
  selected,
  onPick,
  onAddProject,
  onClose,
}: {
  snap: Snapshot;
  selected: string | null;
  onPick(id: string): void;
  onAddProject(): void;
  onClose(): void;
}) {
  const t = useT();
  const [query, setQuery] = useState("");
  const q = query.trim().toLowerCase();
  const projects = snap.projects.filter((p) => !q || p.name.toLowerCase().includes(q));
  return (
    <Popover onClose={onClose}>
      {snap.projects.length > 6 && (
        <div className="menu-search">
          <Search {...ICON} />
          <input
            autoFocus
            value={query}
            placeholder={t("newTask.searchProjects")}
            aria-label={t("newTask.searchProjects")}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && projects[0] && onPick(projects[0].id)}
          />
        </div>
      )}
      <div className="menu-list">
        {projects.map((p) => (
          <button key={p.id} className="menu-item" title={p.root} onClick={() => onPick(p.id)}>
            <Folder {...ICON} className="dim" />
            <span className="ellipsis">{p.name}</span>
            <span className="grow" />
            {p.id === selected && <Check size={13} strokeWidth={2} />}
          </button>
        ))}
      </div>
      <div className="menu-foot column">
        <button className="menu-item" onClick={onAddProject}>
          <FolderPlus {...ICON} className="dim" />
          <span>{t("addProject.title")}…</span>
        </button>
      </div>
    </Popover>
  );
}

/** 选仓库：项目里的仓库；可以新建一个仓库，项目文件夹本身还不是仓库时也可以直接把它变成仓库。 */
function RepoMenu({
  project,
  repos,
  selected,
  label,
  onPick,
  onCreated,
  onError,
  onClose,
}: {
  project: string;
  repos: Repo[];
  selected: string | null;
  label(r: Repo): string;
  onPick(path: string): void;
  onCreated(path: string): void;
  onError(err: unknown): void;
  onClose(): void;
}) {
  const t = useT();
  const [naming, setNaming] = useState(false);
  const [name, setName] = useState("");
  const [busy, setBusy] = useState(false);
  const create = async (repoName: string | null) => {
    setBusy(true);
    try {
      onCreated(await api.addRepo(project, repoName));
    } catch (err) {
      onError(err);
    } finally {
      setBusy(false);
    }
  };
  return (
    <Popover onClose={onClose}>
      <div className="menu-title">{t("newTask.repo")}</div>
      <div className="menu-list">
        {repos.map((r) => (
          <button key={r.path} className="menu-item" title={r.path} onClick={() => onPick(r.path)}>
            <FolderGit2 {...ICON} className="dim" />
            <span className="ellipsis">{label(r)}</span>
            <span className="grow" />
            {r.path === selected && <Check size={13} strokeWidth={2} />}
          </button>
        ))}
        {repos.length === 0 && <div className="menu-empty">{t("newTask.noRepoHint")}</div>}
      </div>
      <div className="menu-foot column">
        {naming ? (
          <form
            className="menu-search"
            onSubmit={(e) => {
              e.preventDefault();
              if (name.trim()) void create(name.trim());
            }}
          >
            <Plus {...ICON} />
            <input
              autoFocus
              value={name}
              placeholder={t("newTask.repoName")}
              aria-label={t("newTask.repoName")}
              onChange={(e) => setName(e.target.value)}
              onKeyDown={(e) => e.key === "Escape" && (e.stopPropagation(), setNaming(false))}
            />
            {busy && <span className="status status-running" />}
          </form>
        ) : (
          <button className="menu-item" onClick={() => setNaming(true)}>
            <Plus {...ICON} className="dim" />
            <span>{t("newTask.newRepo")}</span>
          </button>
        )}
        {repos.length === 0 && (
          <button className="menu-item" disabled={busy} onClick={() => void create(null)}>
            <FolderGit2 {...ICON} className="dim" />
            <span>{t("newTask.initFolder")}</span>
          </button>
        )}
      </div>
    </Popover>
  );
}

/** 当前选的基准分支；没选时显示仓库当前分支。 */
function BranchName({ repo, base }: { repo: string; base: string | null }) {
  const [current, setCurrent] = useState<string | null>(null);
  useEffect(() => {
    setCurrent(null);
    api.branches(repo).then((list) => setCurrent(list[0] ?? null), () => {});
  }, [repo]);
  return <span className="ellipsis">{base ?? current ?? "…"}</span>;
}

function BranchMenu({
  repo,
  selected,
  onPick,
  onClose,
}: {
  repo: string;
  selected: string | null;
  onPick(branch: string): void;
  onClose(): void;
}) {
  const t = useT();
  const [branches, setBranches] = useState<string[] | null>(null);
  const [query, setQuery] = useState("");
  useEffect(() => {
    api.branches(repo).then(setBranches, () => setBranches([]));
  }, [repo]);
  const q = query.trim().toLowerCase();
  const list = (branches ?? []).filter((b) => !q || b.toLowerCase().includes(q)).slice(0, 50);
  const active = selected ?? branches?.[0];
  return (
    <Popover onClose={onClose}>
      <div className="menu-title">{t("newTask.baseBranch")}</div>
      <div className="menu-search">
        <Search {...ICON} />
        <input
          autoFocus
          value={query}
          placeholder={t("newTask.searchBranches")}
          aria-label={t("newTask.searchBranches")}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && list[0] && onPick(list[0])}
        />
      </div>
      <div className="menu-list">
        {branches === null && <div className="menu-empty">…</div>}
        {list.map((b) => (
          <button key={b} className="menu-item mono" onClick={() => onPick(b)}>
            <GitBranch size={12} strokeWidth={1.75} className="dim" />
            <span className="ellipsis">{b}</span>
            <span className="grow" />
            {b === active && <Check size={13} strokeWidth={2} />}
          </button>
        ))}
      </div>
    </Popover>
  );
}

/** 本机一个 Agent 都没装：说明要装什么，给出可以复制的安装命令。 */
function NoAgentNotice({ agents, onRecheck }: { agents: AgentOption[]; onRecheck(): void }) {
  const t = useT();
  return (
    <div className="panel-notice no-agent" role="alert">
      <div className="no-agent-head">
        <TriangleAlert size={15} strokeWidth={1.75} className="warn" />
        <span className="grow">{t("newTask.noAgent")}</span>
        <button className="btn small" onClick={onRecheck}>
          {t("newTask.recheck")}
        </button>
      </div>
      {/* 装了同一个账号的桌面客户端的排在前面：这些用户已经有账号，装上登录就能用。 */}
      {agents
        .filter((a) => a.install)
        .sort((a, b) => Number(!!b.app) - Number(!!a.app))
        .map((a) => (
          <div key={a.id} className="install-row">
            <AgentIcon kind={a.id} size={14} />
            <div className="grow install-what">
              {a.app && <span className="install-app">{t("setup.appFound", { app: a.app, agent: a.name })}</span>}
              <code>{a.install}</code>
            </div>
            <CopyCommand command={a.install} />
            {canSetUp(a) && (
              <button className="btn small primary" onClick={() => requestSetup(a.id)}>
                {t(a.tooOld ? "setup.update" : a.canSignIn ? "setup.installAndSignIn" : "setup.install")}
              </button>
            )}
          </div>
        ))}
    </div>
  );
}

/**
 * 有 Agent 可用时的提示（最多一条）：选中的 Agent 还没登录；或者装了某个桌面客户端（ChatGPT、Claude），
 * 对应的 CLI 却没装——用户已经有这个账号，一键装上、登录就能用，不另外收费。
 * 两种都能关掉：查登录的命令也会误报（例如 Codex 配了别家的模型服务，不用 OpenAI 账号，T3 Code 也提到这一点）。
 */
function SetupHint({ agents, writer }: { agents: AgentOption[]; writer: string }) {
  const t = useT();
  const [dismissed, setDismissed] = useState(readDismissed);
  const selected = agents.find(
    (a) => a.id === writer && a.installed && a.signedIn === false && a.canSignIn && !dismissed.includes(`login:${a.id}`),
  );
  const fromApp = agents.find((a) => !a.installed && !a.tooOld && a.app && a.canInstall && !dismissed.includes(a.id));
  const agent = selected ?? fromApp;
  if (!agent) return null;
  const key = selected ? `login:${agent.id}` : agent.id;
  return (
    <div className="panel-notice setup-hint" role="status">
      <AgentIcon kind={agent.id} size={14} />
      <span className="grow">
        {selected
          ? t("setup.notSignedInHint", { agent: agent.name })
          : t("setup.appFound", { app: agent.app, agent: agent.name })}
      </span>
      <button className="btn small primary" onClick={() => requestSetup(agent.id)}>
        {t(selected ? "setup.signIn" : "setup.installAndSignIn")}
      </button>
      <button
        className="icon-btn"
        title={t("setup.dismiss")}
        onClick={() => {
          const next = [...dismissed, key];
          setDismissed(next);
          writeDismissed(next);
        }}
      >
        <X size={13} strokeWidth={1.75} />
      </button>
    </div>
  );
}

const DISMISSED_KEY = "duolo.setupHintDismissed";

function readDismissed(): string[] {
  try {
    return JSON.parse(localStorage.getItem(DISMISSED_KEY) ?? "[]") as string[];
  } catch {
    return [];
  }
}

function writeDismissed(ids: string[]) {
  try {
    localStorage.setItem(DISMISSED_KEY, JSON.stringify(ids));
  } catch {
    // 记不住就下次再提示。
  }
}

/**
 * 谁来写：装了的 Agent 一字排开，点一下就换，选中的那个显示名字；在输入框里按 ⇥ / ⇧⇥ 也能换。
 * 没装的（和太旧的）放在最后的"更多"里，带安装命令。
 */
function AgentPicker({
  agents,
  selected,
  menuOpen,
  onMenu,
  onPick,
}: {
  agents: AgentOption[];
  selected: string;
  menuOpen: boolean;
  onMenu(open: boolean): void;
  onPick(id: string): void;
}) {
  const t = useT();
  const installed = agents.filter((a) => a.installed);
  const others = agents.length > installed.length;
  return (
    <div className="agent-picker" role="radiogroup" aria-label={t("newTask.writerTitle")} title={t("newTask.writerHint")}>
      {installed.map((a) => (
        <button
          key={a.id}
          role="radio"
          aria-checked={a.id === selected}
          className={`agent-pill ${a.id === selected ? "is-on" : ""}`}
          title={t("newTask.writer", { agent: a.name })}
          onClick={() => onPick(a.id)}
        >
          <AgentIcon kind={a.id} size={14} />
          {a.id === selected && <span>{a.name}</span>}
        </button>
      ))}
      {/* 不用 AI：在任务的工作目录里运行一条命令，或者只开一个终端。 */}
      <button
        role="radio"
        aria-checked={selected === "shell"}
        className={`agent-pill ${selected === "shell" ? "is-on" : ""}`}
        title={t("newTask.shell")}
        onClick={() => onPick("shell")}
      >
        <AgentIcon kind="shell" size={14} />
        {selected === "shell" && <span>{t("agent.shell")}</span>}
      </button>
      {others && (
        <div className="dropdown-anchor">
          <button
            className={`icon-btn agent-more ${menuOpen ? "is-open" : ""}`}
            title={t("newTask.moreAgents")}
            onClick={() => onMenu(!menuOpen)}
          >
            <Ellipsis {...ICON} />
          </button>
          {menuOpen && (
            <AgentMenu
              title={t("newTask.writerTitle")}
              agents={agents}
              selected={selected}
              onPick={onPick}
              onClose={() => onMenu(false)}
            />
          )}
        </div>
      )}
    </div>
  );
}
