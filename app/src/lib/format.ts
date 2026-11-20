import type { Agent, Project, Snapshot, Status, Task } from "./api";
import type { MessageKey } from "../i18n";

/** Agent 状态对应的文案键。 */
export const statusKey = (s: Status) => `status.${s}` as MessageKey;


export const isLive = (s: Status) => s === "running" || s === "needs_input" || s === "idle";

/** 任务的汇总状态：有一个等你就是等你，否则有一个在跑就是运行中。 */
export function taskStatus(task: Task): Status {
  const all = task.agents.map((a) => a.status);
  for (const s of ["needs_input", "running", "idle"] as const) {
    if (all.includes(s)) return s;
  }
  return all[0] ?? "not_started";
}

/** 3s / 4m / 2h / 5d */
export function elapsed(fromSecs: number, nowSecs = Date.now() / 1000): string {
  const s = Math.max(0, Math.floor(nowSecs - fromSecs));
  if (s < 60) return `${s}s`;
  if (s < 3600) return `${Math.floor(s / 60)}m`;
  if (s < 86400) return `${Math.floor(s / 3600)}h`;
  return `${Math.floor(s / 86400)}d`;
}

export function findTask(snap: Snapshot | null, id: number | null): { task: Task; project: Project } | null {
  if (!snap || id == null) return null;
  for (const project of snap.projects) {
    const task = project.tasks.find((t) => t.id === id);
    if (task) return { task, project };
  }
  return null;
}

export const allTasks = (snap: Snapshot | null): Task[] => snap?.projects.flatMap((p) => p.tasks) ?? [];

export const writerOf = (task: Task): Agent | undefined => task.agents.find((a) => a.role === "writer");
export const reviewerOf = (task: Task): Agent | undefined => task.agents.find((a) => a.role === "reviewer");
