import { useCallback, useEffect, useRef, useState } from "react";

import { api, type Agent, type Diff, type Task } from "../lib/api";
import { ROLE_LABEL, isLive } from "../lib/format";
import * as terminals from "../lib/terminals";
import type { Voice } from "../lib/voice";
import { StatusIcon } from "./StatusIcon";
import { VoiceOverlay } from "./VoiceBar";

interface Props {
  task: Task;
  agentId: number;
  hasReviewer: boolean;
  voice: Voice;
  onSelectAgent(agent: number): void;
  onReview(): void;
  onClose(): void;
  onError(err: unknown): void;
}

/** 任务页：顶部是任务和 Agent 切换，下面是 Agent 自己的 CLI。 */
export function TaskView({ task, agentId, hasReviewer, voice, onSelectAgent, onReview, onClose, onError }: Props) {
  const agent = task.agents.find((a) => a.id === agentId) ?? task.agents[0];
  const diff = useDiff(task.id);
  const [menu, setMenu] = useState(false);
  const focusTerminal = useCallback(() => terminals.terminalFor({ task: task.id, agent: agent.id }).term.focus(), [task.id, agent.id]);

  return (
    <div className="task">
      <header className="task-head" data-tauri-drag-region>
        <div className="task-title ellipsis" title={task.prompt} data-tauri-drag-region>
          {task.title}
        </div>
        <nav className="tabs">
          {task.agents.map((a) => (
            <button key={a.id} className={`tab ${a.id === agent.id ? "is-active" : ""}`} onClick={() => onSelectAgent(a.id)}>
              <StatusIcon status={a.status} />
              {a.name}
              <span className="meta">{ROLE_LABEL[a.role]}</span>
            </button>
          ))}
          {!hasReviewer && (
            <button className="tab add" onClick={onReview} title="让 Codex 只读审阅这个任务的改动（⌘⇧R）">
              + 审阅
            </button>
          )}
        </nav>
        <span className="grow" data-tauri-drag-region />
        <div className="task-meta" data-tauri-drag-region>
          <span className="branch" title={`基于 ${task.base}`}>
            {task.branch}
          </span>
          {diff && diff.files > 0 && (
            <span className="diff">
              <span className="ins">+{diff.insertions}</span> <span className="del">−{diff.deletions}</span>
            </span>
          )}
        </div>
        <div className="more" onMouseDown={(e) => e.stopPropagation()}>
          <button className="icon-btn" title="更多" onClick={() => setMenu((m) => !m)}>
            ⋯
          </button>
          {menu && (
            <MoreMenu
              onDismiss={() => setMenu(false)}
              items={[
                { label: "在 Finder 中显示", kbd: "⌘⇧F", run: () => void api.reveal(task.worktree) },
                { label: "在终端中打开", run: () => void api.reveal(task.worktree, "Terminal") },
                { label: "拷贝工作目录路径", run: () => void navigator.clipboard.writeText(task.worktree) },
                { sep: true },
                { label: hasReviewer ? "重新打开审阅" : "让 Codex 审阅", kbd: "⌘⇧R", run: onReview },
                { label: "关闭任务…", kbd: "⌘⇧W", run: onClose, danger: true },
              ]}
            />
          )}
        </div>
      </header>

      <div className="task-body">
        <AgentTerminal key={agent.id} task={task} agent={agent} onError={onError} />
        <VoiceOverlay voice={voice} onDone={focusTerminal} />
      </div>
    </div>
  );
}

/** Agent 的 CLI。没有运行时自动启动（第一次带需求，之后接着上次的会话）。 */
function AgentTerminal({ task, agent, onError }: { task: Task; agent: Agent; onError(err: unknown): void }) {
  const host = useRef<HTMLDivElement>(null);
  const ref = { task: task.id, agent: agent.id };
  const [starting, setStarting] = useState(false);
  const live = isLive(agent.status);
  const ended = agent.status === "exited" || agent.status === "failed";

  // 挂上常驻的 xterm，并跟随窗口大小调整。
  useEffect(() => {
    const el = host.current;
    if (!el) return;
    const unmount = terminals.mount(ref, el);
    let last = "";
    const resize = () => {
      const size = terminals.fit(ref);
      if (!size) return;
      const key = `${size.rows}x${size.cols}`;
      if (key !== last) {
        last = key;
        void api.resize(size.rows, size.cols);
      }
    };
    resize();
    terminals.terminalFor(ref).term.focus();
    const observer = new ResizeObserver(resize);
    observer.observe(el);
    return () => {
      observer.disconnect();
      unmount();
    };
  }, [task.id, agent.id]);

  const start = useCallback(async () => {
    setStarting(true);
    try {
      await api.startAgent(ref);
      terminals.terminalFor(ref).term.focus();
    } catch (err) {
      onError(err);
    } finally {
      setStarting(false);
    }
  }, [task.id, agent.id, onError]);

  // 没有进程就启动；有进程就订阅它的输出（进程换了会重新订阅）。
  useEffect(() => {
    if (agent.session != null) {
      terminals.attach(ref, agent.session).catch(onError);
    } else if (agent.status === "not_started" && !starting) {
      void start();
    }
  }, [agent.session, agent.status, task.id, agent.id]);

  return (
    <div className="terminal-wrap">
      <div ref={host} className="terminal" />
      {(starting || agent.status === "not_started") && !live && (
        <div className="terminal-note">正在启动 {agent.name}…</div>
      )}
      {ended && (
        <div className="terminal-ended">
          <span>
            {agent.name} {agent.status === "failed" ? "异常退出" : "已结束"}
          </span>
          <button className="btn" onClick={() => void start()} disabled={starting}>
            继续会话
          </button>
        </div>
      )}
    </div>
  );
}

type MenuItem = { label: string; kbd?: string; run(): void; danger?: boolean } | { sep: true };

function MoreMenu({ items, onDismiss }: { items: MenuItem[]; onDismiss(): void }) {
  useEffect(() => {
    window.addEventListener("mousedown", onDismiss);
    return () => window.removeEventListener("mousedown", onDismiss);
  }, [onDismiss]);
  return (
    <div className="menu more-menu">
      {items.map((item, i) =>
        "sep" in item ? (
          <div key={i} className="menu-sep" />
        ) : (
          <button
            key={i}
            className={`menu-item ${item.danger ? "danger" : ""}`}
            onClick={() => {
              onDismiss();
              item.run();
            }}
          >
            <span className="grow">{item.label}</span>
            {item.kbd && <kbd>{item.kbd}</kbd>}
          </button>
        ),
      )}
    </div>
  );
}

/** 每 3 秒刷新一次改动统计。 */
function useDiff(task: number) {
  const [diff, setDiff] = useState<Diff | null>(null);
  useEffect(() => {
    let alive = true;
    const load = () => api.diffStat(task).then((d) => alive && setDiff(d), () => {});
    void load();
    const id = window.setInterval(load, 3000);
    return () => {
      alive = false;
      window.clearInterval(id);
    };
  }, [task]);
  return diff;
}
