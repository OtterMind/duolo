// 后端命令的类型化封装。类型与 crates/duolo-app/src/view.rs 保持一致。

import { Channel, invoke } from "@tauri-apps/api/core";

export type Status = "not_started" | "running" | "needs_input" | "idle" | "exited" | "failed";

/** 任务在列表上的状态：写代码的 Agent 和审阅合在一起，由后端算好（duolo-core/src/wire.rs 的 TaskState），和手机端一致。 */
export type TaskState =
  | "needs_you"
  | "issues"
  | "review_failed"
  | "crashed"
  | "writing"
  | "reviewing"
  | "passed"
  | "reviewed"
  | "written"
  | "ended"
  | "not_started";
export type Role = "writer" | "reviewer";

export interface Agent {
  id: number;
  kind: string;
  name: string;
  role: Role;
  status: Status;
  /** 当前进程的编号，变化时要重新订阅输出。 */
  session: number | null;
  /** 在哪里运行：它自己的命令行，或者 Duolo 画的对话。 */
  surface: Surface;
  /** 能不能在聊天模式下运行。 */
  chat: boolean;
  /** 聊天模式下正在写的话、在想、在用的工具（还没记成事件）。 */
  draft: Draft | null;
  /** 聊天模式下出错的原因（Agent 自己说的）。 */
  error: string | null;
}

export type Surface = "terminal" | "chat";

export interface Draft {
  text: string;
  thinking: boolean;
  tool: string | null;
  target: string | null;
}

export interface Task {
  id: number;
  title: string;
  prompt: string;
  /** 还没有需求、也没改过名的空白任务。 */
  untitled: boolean;
  branch: string;
  base: string;
  worktree: string;
  createdAt: number;
  agents: Agent[];
  /** 审阅用的 Agent。 */
  reviewer: { kind: string; name: string };
  /** 改的是项目里的哪个仓库（相对项目文件夹）；项目本身就是仓库时为空。 */
  repo: string;
  /** 正在后台审阅时的开始时间（Unix 秒）。 */
  reviewingSince: number | null;
  review: Review | null;
  state: TaskState;
  /** 换回来时能接着原来会话的 Agent。 */
  resumable: string[];
  /** 从哪个任务分出来的。 */
  forkedFrom: number | null;
}

export interface Review {
  finishedAt: number;
  ok: boolean;
  /** 审阅结论的判断：通过、有必须修改的问题、没有标记。 */
  verdict: "pass" | "issues" | "unknown";
  /** 必须修改的问题数（verdict 为 issues 时）。 */
  issues: number;
  /** 结论第一行。 */
  summary: string;
  output: string;
}

export interface Project {
  id: string;
  name: string;
  root: string;
  tasks: Task[];
}

export interface Snapshot {
  projects: Project[];
  running: number;
  needsInput: number;
}

export interface Check {
  kind: "git" | "agent" | "voice";
  /** Agent 的种类，其他检查为空。 */
  agent: string;
  /** 没安装时的安装命令（Agent 才有）。 */
  install: string;
  /** 显示名，例如 git、Agent 的名字。 */
  name: string;
  ok: boolean;
  command: string;
  version: string;
  /** 装的版本太旧时，最低要求的版本。 */
  minimum: string;
  /** 本机装了的、用同一个账号的桌面客户端，没有时为空。 */
  app: string;
  /** 有没有登录；不知道时为 null。 */
  signedIn: boolean | null;
  /** 能一键安装、太旧时能一键升级、能一键登录。 */
  canInstall: boolean;
  canUpdate: boolean;
  canSignIn: boolean;
}

/** 一键安装、升级 Agent，或者登录它的账号。 */
export type SetupStep = "install" | "update" | "login";

/** 一种 Agent 的名字和图标。 */
export interface AgentPlugin {
  id: string;
  name: string;
  /** 单色 SVG，没有时显示首字母。 */
  icon: string | null;
  /** 图标的颜色，没有时跟随文字颜色。 */
  color: string | null;
  /** 能不能用来审阅。 */
  reviews: boolean;
  /** 能不能在聊天模式下运行。 */
  chat: boolean;
}

/** 关闭任务的结果。 */
export interface Closed {
  /** 没提交的改动保存到了哪个分支。 */
  savedTo: string | null;
  /** 工作目录没法安全删除，留在了这里。 */
  leftAt: string | null;
}

export interface Diff {
  files: number;
  insertions: number;
  deletions: number;
}

/** 一个 git 仓库。在项目里时 `name` 是相对项目文件夹的路径（项目本身就是仓库时为空）。 */
export interface Repo {
  name: string;
  path: string;
}

/** 任务里发生的一件事（和 crates/duolo-core/src/event.rs 一致）。手机端也用同一套格式。 */
export type TaskEvent = { task: number; seq: number; at: number } & (
  | { type: "task_created" | "task_renamed"; title: string; branch: string }
  | { type: "task_closed" }
  | { type: "message"; speaker: "user" | "agent"; agent: number; text: string }
  | { type: "tool"; agent: number; tool: string; target: string }
  | { type: "status"; agent: number; status: Status }
  | { type: "prompt"; agent: number; question: string; detail: string; options: string[] }
  | { type: "approval"; agent: number; tool: string; target: string }
  | { type: "writer_changed"; from: number; to: number; kind: string }
  | { type: "reviewer_changed"; reviewer: string }
  | { type: "review_started"; reviewer: string }
  | { type: "review_finished"; ok: boolean; verdict: Review["verdict"]; issues: number; summary: string }
  | { type: "diff"; files: number; insertions: number; deletions: number }
);

/** 远程命令（和 crates/duolo-core/src/event.rs 的 Command 一致）。 */
export type RemoteCommand =
  | ({ type: "create_task"; project: string; prompt: string } & NewTaskOptions)
  | { type: "send_message"; task: number; text: string }
  | { type: "answer"; task: number; option: number; prompt?: number }
  | { type: "interrupt" | "start_review" | "close_task"; task: number }
  | { type: "switch_writer" | "set_reviewer"; task: number; agent: string };

export interface CommandResult {
  ok: boolean;
  task?: number;
  /** 错误码，对应语言包里的 error.*。 */
  error?: string;
}

/** 新建任务的选项，没给的按默认。 */
export interface NewTaskOptions {
  /** 项目里的哪个仓库（路径）。 */
  repo?: string | null;
  /** 基于哪个分支。 */
  base?: string | null;
  /** 新分支名。 */
  branch?: string | null;
  writer?: string;
  reviewer?: string;
  /** 在终端里还是聊天模式下运行，默认和上次一样。 */
  surface?: Surface;
}

export interface VoiceStatus {
  text: string;
  /** 0~1；还没收到声音时为 null。 */
  level: number | null;
}

export interface VoiceSettings {
  organize: boolean;
}

export interface FnKeyStatus {
  /** 按住 Fn 说话已经生效。 */
  active: boolean;
  /** 已有辅助功能权限。 */
  trusted: boolean;
  /** 系统里按 🌐 键会弹出表情或切换输入法，和按住 Fn 冲突。 */
  conflict: boolean;
}

export type SettingsPane = "keyboard" | "accessibility" | "microphone" | "speech" | "sound";

export interface VoiceResult {
  raw: string;
  organized: string | null;
}

/** 一个 Agent 的唯一标识。 */
export interface AgentRef {
  task: number;
  agent: number;
}

export const agentKey = (r: AgentRef) => `${r.task}:${r.agent}`;

export const api = {
  snapshot: () => invoke<Snapshot>("snapshot"),
  quitSeen: () => invoke<void>("quit_seen"),
  startupNotices: () => invoke<unknown[]>("startup_notices"),
  environment: () => invoke<Check[]>("environment"),
  takeOpenRequest: () => invoke<string | null>("take_open_request"),
  addProject: (path: string) => invoke<string>("add_project", { path }),
  createTask: (project: string, prompt: string, opts: NewTaskOptions) =>
    invoke<number>("create_task", {
      project,
      prompt,
      repo: opts.repo ?? null,
      base: opts.base ?? null,
      branch: opts.branch ?? null,
      writer: opts.writer || null,
      reviewer: opts.reviewer || null,
      surface: opts.surface ?? null,
    }),
  taskEvents: (task: number, after: number) => invoke<TaskEvent[]>("task_events", { task, after }),
  execute: (command: RemoteCommand) => invoke<CommandResult>("execute", { command }),
  suggestBranch: (prompt: string) => invoke<string | null>("suggest_branch", { prompt }),
  renameBranch: (task: number, branch: string) => invoke<void>("rename_branch", { task, branch }),
  projectRepos: (project: string) => invoke<Repo[]>("project_repos", { project }),
  addRepo: (project: string, name: string | null) => invoke<string>("add_repo", { project, name }),
  projectsHome: () => invoke<string>("projects_home"),
  createProjectFolder: (name: string, parent: string | null) =>
    invoke<string>("create_project_folder", { name, parent }),
  agentPlugins: () => invoke<AgentPlugin[]>("agent_plugins"),
  agentDefaults: () => invoke<{ writer: string; reviewer: string; surface: Surface }>("agent_defaults"),
  renameTask: (task: number, title: string) => invoke<void>("rename_task", { task, title }),
  branches: (repo: string) => invoke<string[]>("branches", { repo }),
  discoverRepos: () => invoke<Repo[]>("discover_repos"),
  cloneProject: (url: string) => invoke<string>("clone_project", { url }),
  nameTask: (task: number, prompt: string) => invoke<void>("name_task", { task, prompt, branch: null }),
  closeTask: (task: number) => invoke<Closed>("close_task", { task }),
  startAgent: (r: AgentRef) => invoke<void>("start_agent", { ...r }),
  /** 换写代码的 Agent，返回接手的 Agent 的编号。 */
  switchWriter: (task: number, kind: string, request?: string) =>
    invoke<number>("switch_writer", { task, kind, request }),
  /** 分出新任务：照着现在的代码新建工作区，`kind` 带着交接说明接着做，原来的任务不停。返回新任务。 */
  forkTask: (task: number, kind: string, request: string) => invoke<number>("fork_task", { task, kind, request }),
  /** 换个方式运行写代码的 Agent，接着同一个会话。 */
  setSurface: (task: number, surface: Surface) => invoke<void>("set_surface", { task, surface }),
  setReviewer: (task: number, kind: string) => invoke<void>("set_reviewer", { task, kind }),
  /** 叫 `reviewer`（不给就是上次叫的）在后台审阅任务的改动。 */
  startReview: (task: number, reviewer?: string) => invoke<void>("start_review", { task, reviewer }),
  cancelReview: (task: number) => invoke<void>("cancel_review", { task }),
  write: (r: AgentRef, data: string) => invoke<void>("write", { ...r, data }),
  newline: (r: AgentRef) => invoke<void>("newline", { ...r }),
  sendText: (r: AgentRef, text: string, submit: boolean) => invoke<void>("send_text", { ...r, text, submit }),
  resize: (r: AgentRef, rows: number, cols: number) => invoke<void>("resize", { ...r, rows, cols }),
  diffStat: (task: number) => invoke<Diff | null>("diff_stat", { task }),
  reveal: (path: string) => invoke<void>("reveal", { path }),
  openTerminal: (path: string) => invoke<void>("open_terminal", { path }),
  /** 连上服务器开始录音；`task` 用来带上热词和最近的对话。 */
  voiceStart: (task?: number, project?: string) => invoke<void>("voice_start", { task, project }),
  voiceCancel: () => invoke<void>("voice_cancel"),
  voiceStatus: () => invoke<VoiceStatus>("voice_status"),
  getVoiceSettings: () => invoke<VoiceSettings>("get_voice_settings"),
  setVoiceSettings: (voice: VoiceSettings) => invoke<void>("set_voice_settings", { voice }),
  fnKeyStatus: () => invoke<FnKeyStatus>("fn_key_status"),
  requestFnKeyPermission: () => invoke<void>("request_fn_key_permission"),
  openSystemSettings: (pane: SettingsPane) => invoke<void>("open_system_settings", { pane }),
  voiceFinish: () => invoke<VoiceResult>("voice_finish"),
  quit: () => invoke<void>("quit"),

  /** 订阅 Agent 输出：先收到已有输出的重放，之后是实时输出。 */
  attach(r: AgentRef, onData: (bytes: Uint8Array) => void) {
    const channel = new Channel<ArrayBuffer | number[]>();
    channel.onmessage = (data) => onData(data instanceof ArrayBuffer ? new Uint8Array(data) : Uint8Array.from(data));
    return invoke<void>("attach", { ...r, onData: channel });
  },

  /** 在单独的终端里安装、升级 Agent 或登录账号，返回这是第几次；结束时发带着这个编号的 `setup-exit` 事件。 */
  setupStart(agent: string, step: SetupStep, rows: number, cols: number, onData: (bytes: Uint8Array) => void) {
    const channel = new Channel<ArrayBuffer | number[]>();
    channel.onmessage = (data) => onData(data instanceof ArrayBuffer ? new Uint8Array(data) : Uint8Array.from(data));
    return invoke<number>("setup_start", { agent, step, rows, cols, onData: channel });
  },
  /** 现在登录了没有，不知道时为 null。 */
  setupSignedIn: (agent: string) => invoke<boolean | null>("setup_signed_in", { agent }),
  setupWrite: (data: string) => invoke<void>("setup_write", { data }),
  setupResize: (rows: number, cols: number) => invoke<void>("setup_resize", { rows, cols }),
  setupStop: () => invoke<void>("setup_stop"),
};
