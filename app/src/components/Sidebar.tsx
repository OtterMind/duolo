import { useEffect, useState } from "react";

import type { Snapshot, Task } from "../lib/api";
import { ROLE_LABEL, STATUS_LABEL, elapsed, taskStatus } from "../lib/format";
import type { View } from "../App";
import { StatusIcon } from "./StatusIcon";

interface Props {
  snap: Snapshot | null;
  view: View;
  onNewTask(project?: string): void;
  onOpenTask(task: number, agent?: number): void;
  onCloseTask(task: number): void;
  onAddProject(): void;
}

export function Sidebar({ snap, view, onNewTask, onOpenTask, onCloseTask, onAddProject }: Props) {
  const now = useNow();
  const projects = snap?.projects ?? [];

  return (
    <aside className="sidebar">
      <div className="sidebar-top" data-tauri-drag-region />
      <button className={`side-row new-task ${view.kind === "home" ? "is-current" : ""}`} onClick={() => onNewTask()}>
        <span className="plus">+</span>
        <span className="grow">新任务</span>
        <kbd>⌘N</kbd>
      </button>

      <div className="side-scroll">
        {projects.map((project) => (
          <section key={project.id} className="side-project">
            <div className="side-project-head">
              <span className="grow" title={project.root}>
                {project.name}
              </span>
              <button className="icon-btn" title={`在 ${project.name} 新建任务`} onClick={() => onNewTask(project.id)}>
                +
              </button>
            </div>
            {project.tasks.length === 0 && <div className="side-empty">还没有任务</div>}
            {project.tasks.map((task) => (
              <TaskRows
                key={task.id}
                task={task}
                now={now}
                view={view}
                onOpen={onOpenTask}
                onClose={onCloseTask}
              />
            ))}
          </section>
        ))}
        {snap && projects.length === 0 && <div className="side-empty">还没有项目</div>}
        <button className="side-row add-project" onClick={onAddProject}>
          <span className="plus">+</span>
          <span className="grow">添加项目</span>
        </button>
      </div>

      <footer className="sidebar-foot">
        {!snap || snap.running + snap.needsInput === 0 ? (
          <span>没有运行中的 Agent</span>
        ) : (
          <span>
            {snap.running} 个运行中
            {snap.needsInput > 0 && <span className="need"> · {snap.needsInput} 个等你</span>}
          </span>
        )}
      </footer>
    </aside>
  );
}

function TaskRows({
  task,
  now,
  view,
  onOpen,
  onClose,
}: {
  task: Task;
  now: number;
  view: View;
  onOpen(task: number, agent?: number): void;
  onClose(task: number): void;
}) {
  const isCurrent = view.kind === "task" && view.task === task.id;
  return (
    <div className={`side-task ${isCurrent ? "is-open" : ""}`}>
      <div
        className={`side-row task-row ${isCurrent ? "is-current" : ""}`}
        role="button"
        tabIndex={0}
        onClick={() => onOpen(task.id)}
        title={task.prompt}
      >
        <StatusIcon status={taskStatus(task)} />
        <span className="grow ellipsis">{task.title}</span>
        <span className="meta when">{elapsed(task.createdAt, now)}</span>
        <button
          className="icon-btn close"
          title="关闭任务"
          onClick={(e) => {
            e.stopPropagation();
            onClose(task.id);
          }}
        >
          ×
        </button>
      </div>
      {task.agents.map((agent) => {
        const active = isCurrent && view.kind === "task" && view.agent === agent.id;
        return (
          <div
            key={agent.id}
            className={`side-row agent-row ${active ? "is-active" : ""}`}
            role="button"
            tabIndex={0}
            onClick={() => onOpen(task.id, agent.id)}
          >
            <StatusIcon status={agent.status} />
            <span className="agent-name">{agent.name}</span>
            <span className="meta">{ROLE_LABEL[agent.role]}</span>
            <span className="grow" />
            <span className={`meta ${agent.status === "needs_input" ? "need" : ""}`}>{STATUS_LABEL[agent.status]}</span>
          </div>
        );
      })}
    </div>
  );
}

/** 每秒刷新一次的当前时间（秒），用来显示任务用时。 */
function useNow() {
  const [now, setNow] = useState(() => Date.now() / 1000);
  useEffect(() => {
    const id = window.setInterval(() => setNow(Date.now() / 1000), 1000);
    return () => window.clearInterval(id);
  }, []);
  return now;
}
