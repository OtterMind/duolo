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
  branch: string;
  base: string;
  worktree: string;
  createdAt: number;
  agents: Agent[];
  /** 正在后台审阅时的开始时间（Unix 秒）。 */
  reviewingSince: number | null;
  review: Review | null;
}

export interface Review {
  finishedAt: number;
  ok: boolean;
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
  /** 显示名，例如 git、Claude。 */
  name: string;
  ok: boolean;
  command: string;
  version: string;
}

export interface Diff {
  files: number;
  insertions: number;
  deletions: number;
}

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
  environment: () => invoke<Check[]>("environment"),
  takeOpenRequest: () => invoke<string | null>("take_open_request"),
  addProject: (path: string) => invoke<string>("add_project", { path }),
  createTask: (project: string, prompt: string) => invoke<number>("create_task", { project, prompt }),
  nameTask: (task: number, prompt: string) => invoke<void>("name_task", { task, prompt }),
  closeTask: (task: number) => invoke<void>("close_task", { task }),
  startAgent: (r: AgentRef) => invoke<void>("start_agent", { ...r }),
  startReview: (task: number) => invoke<void>("start_review", { task }),
  cancelReview: (task: number) => invoke<void>("cancel_review", { task }),
  write: (r: AgentRef, data: string) => invoke<void>("write", { ...r, data }),
  sendText: (r: AgentRef, text: string, submit: boolean) => invoke<void>("send_text", { ...r, text, submit }),
  resize: (rows: number, cols: number) => invoke<void>("resize", { rows, cols }),
  diffStat: (task: number) => invoke<Diff | null>("diff_stat", { task }),
  reveal: (path: string, app?: string) => invoke<void>("reveal", { path, app }),
  voiceStart: () => invoke<void>("voice_start"),
  voiceCancel: () => invoke<void>("voice_cancel"),
  voicePartial: () => invoke<string>("voice_partial"),
  voiceFinish: (project?: string, task?: string) => invoke<VoiceResult>("voice_finish", { project, task }),
  quit: () => invoke<void>("quit"),

  /** 订阅 Agent 输出：先收到已有输出的重放，之后是实时输出。 */
  attach(r: AgentRef, onData: (bytes: Uint8Array) => void) {
    const channel = new Channel<ArrayBuffer | number[]>();
    channel.onmessage = (data) => onData(data instanceof ArrayBuffer ? new Uint8Array(data) : Uint8Array.from(data));
    return invoke<void>("attach", { ...r, onData: channel });
  },
};
