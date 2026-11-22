// 后端命令的类型化封装。类型与 crates/duolo-app/src/view.rs 保持一致。

import { Channel, invoke } from "@tauri-apps/api/core";

export type Status = "not_started" | "running" | "needs_input" | "idle" | "exited" | "failed";
export type Role = "writer" | "reviewer";

export interface Agent {
  id: number;
  kind: string;
  name: string;
  role: Role;
  status: Status;
  /** 当前进程的编号，变化时要重新订阅输出。 */
  session: number | null;
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
  /** Agent 的种类（claude、codex），其他检查为空。 */
  agent: string;
  /** 没安装时的安装命令（Agent 才有）。 */
  install: string;
  /** 显示名，例如 git、Claude。 */
  name: string;
  ok: boolean;
  command: string;
  version: string;
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
  | { type: "review_started"; reviewer: string }
  | { type: "review_finished"; ok: boolean; verdict: Review["verdict"]; issues: number; summary: string }
  | { type: "diff"; files: number; insertions: number; deletions: number }
);

/** 远程命令（和 crates/duolo-core/src/event.rs 的 Command 一致）。 */
export type RemoteCommand =
  | ({ type: "create_task"; project: string; prompt: string } & NewTaskOptions)
  | { type: "send_message"; task: number; text: string }
  | { type: "answer"; task: number; option: number }
  | { type: "interrupt" | "start_review" | "close_task"; task: number };

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
}

export interface VoiceStatus {
  text: string;
  /** 0~1；识别程序不报告音量时为 null。 */
  level: number | null;
}

export interface VoiceSettings {
  /** `auto`（跟随界面语言）或语言代码。 */
  language: string;
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
      writer: opts.writer ?? null,
      reviewer: opts.reviewer ?? null,
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
  agentDefaults: () => invoke<{ writer: string; reviewer: string }>("agent_defaults"),
  renameTask: (task: number, title: string) => invoke<void>("rename_task", { task, title }),
  branches: (repo: string) => invoke<string[]>("branches", { repo }),
  discoverRepos: () => invoke<Repo[]>("discover_repos"),
  cloneProject: (url: string) => invoke<string>("clone_project", { url }),
  nameTask: (task: number, prompt: string) => invoke<void>("name_task", { task, prompt, branch: null }),
  closeTask: (task: number) => invoke<Closed>("close_task", { task }),
  startAgent: (r: AgentRef) => invoke<void>("start_agent", { ...r }),
  startReview: (task: number) => invoke<void>("start_review", { task }),
  cancelReview: (task: number) => invoke<void>("cancel_review", { task }),
  write: (r: AgentRef, data: string) => invoke<void>("write", { ...r, data }),
  sendText: (r: AgentRef, text: string, submit: boolean) => invoke<void>("send_text", { ...r, text, submit }),
  resize: (rows: number, cols: number) => invoke<void>("resize", { rows, cols }),
  diffStat: (task: number) => invoke<Diff | null>("diff_stat", { task }),
  reveal: (path: string) => invoke<void>("reveal", { path }),
  openTerminal: (path: string) => invoke<void>("open_terminal", { path }),
  voiceStart: () => invoke<void>("voice_start"),
  voiceCancel: () => invoke<void>("voice_cancel"),
  voiceStatus: () => invoke<VoiceStatus>("voice_status"),
  getVoiceSettings: () => invoke<VoiceSettings>("get_voice_settings"),
  setVoiceSettings: (voice: VoiceSettings) => invoke<void>("set_voice_settings", { voice }),
  fnKeyStatus: () => invoke<FnKeyStatus>("fn_key_status"),
  requestFnKeyPermission: () => invoke<void>("request_fn_key_permission"),
  openSystemSettings: (pane: SettingsPane) => invoke<void>("open_system_settings", { pane }),
  voiceFinish: (project?: string, task?: string) => invoke<VoiceResult>("voice_finish", { project, task }),
  quit: () => invoke<void>("quit"),

  /** 订阅 Agent 输出：先收到已有输出的重放，之后是实时输出。 */
  attach(r: AgentRef, onData: (bytes: Uint8Array) => void) {
    const channel = new Channel<ArrayBuffer | number[]>();
    channel.onmessage = (data) => onData(data instanceof ArrayBuffer ? new Uint8Array(data) : Uint8Array.from(data));
    return invoke<void>("attach", { ...r, onData: channel });
  },
};
