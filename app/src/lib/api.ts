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
export type Role = "writer" | "reviewer" | "shell";

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
  /** 由哪个定时任务自动建的。 */
  automation: number | null;
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

/** 克隆的进度：哪一步、这一步的百分比，下载时还有已下载的大小和速度。 */
export interface CloneProgress {
  phase: "connecting" | "preparing" | "receiving" | "resolving" | "checkingOut";
  percent: number | null;
  detail: string;
}

/** 定时任务：按时间表交给 Agent 一个需求，每次在项目里新建任务，或者发进一个已有的任务。 */
export interface Automation {
  id: number;
  name: string;
  /** 在哪个项目（不针对哪个项目的是 `Snapshot.automationsProject`）。 */
  project: string;
  /** 发进这个任务接着对话；null 时每次新建任务。 */
  task: number | null;
  prompt: string;
  /** 写代码的 Agent；null 时用默认的。 */
  agent: string | null;
  base: string | null;
  /** 5 段 cron，按这台电脑的时区。 */
  cron: string;
  enabled: boolean;
  /** 下一次（Unix 秒）；暂停了或时间表算不出来时为 null。 */
  nextRunAt: number | null;
  runRequested: boolean;
  /** 最近的执行，新的在前。 */
  runs: AutomationRun[];
}

export interface AutomationRun {
  scheduledFor: number;
  at: number;
  manual: boolean;
  outcome: "starting" | "started" | "skipped" | "failed";
  task: number | null;
  /** 跳过的原因（automation.skip.*）或失败的错误码（error.*）。 */
  code: string | null;
  /** 失败时填进错误文案的参数。 */
  params: Record<string, string>;
}

export interface AutomationInput {
  name: string;
  /** 每次在这个项目里新建任务；null 时不针对哪个项目。 */
  project: string | null;
  /** 发进这个任务接着对话。 */
  task: number | null;
  prompt: string;
  agent: string | null;
  base: string | null;
  cron: string;
}

export interface Snapshot {
  projects: Project[];
  /** 所有定时任务，按建的先后。 */
  automations: Automation[];
  /** 专门放"不针对哪个项目"的定时任务的项目。 */
  automationsProject: string | null;
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
  /** 装了的能一键升级（不管旧不旧），有新版本时用。 */
  updatable: boolean;
}

/** 检查更新的结果。 */
/** 和手机之间的连接（`duolo_link::Status`）。`error` 是错误码。 */
export type LinkStatus =
  | { state: "off" }
  | { state: "connecting" }
  | { state: "online" }
  | { state: "offline"; error: string };

/** 配好对的手机（`duolo_link::PhoneView`）。 */
export interface PhoneView {
  id: string;
  name: string;
  platform: string;
  online: boolean;
  /** Unix 毫秒。 */
  pairedAt: number;
}

/** 给手机扫的配对二维码（`duolo_link::Pairing`）。 */
export interface Pairing {
  link: string;
  /** 二维码（SVG）。 */
  svg: string;
  /** Unix 毫秒。 */
  expiresAt: number;
}

export interface Updates {
  /** Duolo 的新版本，已经是最新时为 null。 */
  duolo: { version: string; url: string } | null;
  /** 没查到 Duolo 有没有新版本（网络不通）。 */
  duoloFailed: boolean;
  /** 各个 Agent 的最新版本，按 Agent 的种类；查不到的不在里面。 */
  agents: Record<string, string>;
}

/** 一键安装、升级 Agent，或者登录它的账号。 */
export type SetupStep = "install" | "update" | "login";

/** 一种 Agent 的一个账号。 */
export interface AgentAccount {
  /** 账号的文件夹，默认账号是空字符串。 */
  id: string;
  /** 从哪里来：`default`、`duolo`，或者别的软件的名字（例如 Orca）。 */
  source: string;
  /** 登录的邮箱，查不出来时为空。 */
  email: string;
  signedIn: boolean | null;
  chosen: boolean;
}

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
  /** 能不能用多个账号。 */
  accounts: boolean;
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
  /** 要认准的词，识别时当热词。 */
  words: string[];
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
  /** 克隆仓库并加为项目，进度交给 `onProgress`；`cancelClone` 取消（报错码 `cancelled`）。 */
  cloneProject: (url: string, onProgress: (p: CloneProgress) => void) => {
    const channel = new Channel<CloneProgress>();
    channel.onmessage = onProgress;
    return invoke<string>("clone_project", { url, onProgress: channel });
  },
  cancelClone: () => invoke<void>("cancel_clone"),
  nameTask: (task: number, prompt: string) => invoke<void>("name_task", { task, prompt, branch: null }),
  closeTask: (task: number) => invoke<Closed>("close_task", { task }),
  /** 移除项目：先关闭它的任务（改动存到各自的分支），再从列表里去掉；磁盘上的文件夹不动。 */
  removeProject: (project: string) => invoke<Closed[]>("remove_project", { project }),
  /** 新建（id 为 null）或修改定时任务，返回它的编号。 */
  saveAutomation: (id: number | null, input: AutomationInput) => invoke<number>("save_automation", { id, input }),
  removeAutomation: (id: number) => invoke<void>("remove_automation", { id }),
  setAutomationEnabled: (id: number, enabled: boolean) => invoke<void>("set_automation_enabled", { id, enabled }),
  /** 立即运行一次，返回新建的任务。 */
  runAutomation: (id: number) => invoke<number[]>("run_automation", { id }),
  /** 时间表下一次的时间（Unix 秒）；不合法时报错。 */
  previewSchedule: (cron: string) => invoke<number | null>("preview_schedule", { cron }),
  startAgent: (r: AgentRef) => invoke<void>("start_agent", { ...r }),
  /** 换写代码的 Agent，返回接手的 Agent 的编号。 */
  switchWriter: (task: number, kind: string, request?: string) =>
    invoke<number>("switch_writer", { task, kind, request }),
  /** 在任务里开一个终端（用户自己的 shell，在任务的工作目录里）。返回它的编号。 */
  openShell: (task: number) => invoke<number>("open_shell", { task }),
  closeShell: (task: number, agent: number) => invoke<void>("close_shell", { task, agent }),
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
  setDefaultReviewer: (agent: string) => invoke<void>("set_default_reviewer", { agent }),
  setDefaultSurface: (surface: Surface) => invoke<void>("set_default_surface", { surface }),
  /** 版本，数据和配置文件在哪里。 */
  /** 检查 Duolo 和各个 Agent 有没有新版本，要联网，几秒。 */
  checkUpdates: () => invoke<Updates>("check_updates"),
  appInfo: () => invoke<{ version: string; dataDir: string; configFile: string }>("app_info"),
  openTerminal: (path: string) => invoke<void>("open_terminal", { path }),
  /** 剪贴板里的文件或图片对应的路径（复制的图片先存成临时 PNG）；只有文字时为空。 */
  clipboardPaths: () => invoke<string[]>("clipboard_paths"),
  /** 用默认浏览器打开网址（只打开 http / https）。 */
  openUrl: (url: string) => invoke<void>("open_url", { url }),
  linkStatus: () => invoke<LinkStatus>("link_status"),
  linkPhones: () => invoke<PhoneView[]>("link_phones"),
  linkPair: () => invoke<Pairing>("link_pair"),
  linkUnpair: (id: string) => invoke<void>("link_unpair", { id }),
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

  /**
   * 在单独的终端里安装、升级 Agent 或登录账号，返回这是第几次（结束时发带着这个编号的 `setup-exit` 事件）。
   * 登录时 `account` 是要登录的账号，`new` 是新添加一个，返回里的 `account` 是新账号的文件夹。
   */
  setupStart(
    agent: string,
    step: SetupStep,
    account: string | undefined,
    rows: number,
    cols: number,
    onData: (bytes: Uint8Array) => void,
  ) {
    const channel = new Channel<ArrayBuffer | number[]>();
    channel.onmessage = (data) => onData(data instanceof ArrayBuffer ? new Uint8Array(data) : Uint8Array.from(data));
    return invoke<{ run: number; account: string | null }>("setup_start", {
      agent,
      step,
      account: account ?? null,
      rows,
      cols,
      onData: channel,
    });
  },
  /** 现在登录了没有（`account` 没有时是默认账号），不知道时为 null。 */
  setupSignedIn: (agent: string, account?: string) =>
    invoke<boolean | null>("setup_signed_in", { agent, account: account ?? null }),
  /** 一种 Agent 的账号和登录状态（要查一会儿）。 */
  agentAccounts: (agent: string) => invoke<AgentAccount[]>("agent_accounts", { agent }),
  chooseAccount: (agent: string, id: string) => invoke<void>("choose_account", { agent, id }),
  removeAccount: (agent: string, id: string) => invoke<void>("remove_account", { agent, id }),
  autoSwitchAccount: () => invoke<boolean>("auto_switch_account"),
  setAutoSwitchAccount: (on: boolean) => invoke<void>("set_auto_switch_account", { on }),
  setupWrite: (data: string) => invoke<void>("setup_write", { data }),
  setupResize: (rows: number, cols: number) => invoke<void>("setup_resize", { rows, cols }),
  setupStop: () => invoke<void>("setup_stop"),
};
