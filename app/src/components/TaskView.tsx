import { useCallback, useEffect, useRef, useState } from "react";
import { Ellipsis, GitBranch, X } from "lucide-react";

import { useT } from "../i18n";

import { api, type Agent, type Diff, type Task } from "../lib/api";
import { elapsed, isLive, writerOf } from "../lib/format";
import * as terminals from "../lib/terminals";
import type { Voice } from "../lib/voice";
import { StatusIcon } from "./StatusIcon";
import { AgentIcon } from "./AgentIcon";
import { Markdown } from "./Markdown";
import { VoiceBar } from "./VoiceBar";
import { isMac, kbd } from "../lib/platform";

interface Props {
  task: Task;
  voice: Voice;
  reviewOpen: boolean;
  /** 这个任务的审阅 Agent 本机装了没有。 */
  reviewerInstalled: boolean;
  onReviewOpenChange(open: boolean): void;
  onClose(): void;
  onError(err: unknown): void;
}

/** 任务页：顶部一行任务信息和审阅入口，下面就是写代码的 Agent 自己的命令行。 */
export function TaskView({ task, voice, reviewOpen, reviewerInstalled, onReviewOpenChange, onClose, onError }: Props) {
  const t = useT();
  const agent = writerOf(task) ?? task.agents[0];
  const ref = { task: task.id, agent: agent.id };
  const diff = useDiff(task.id);
  const [menu, setMenu] = useState(false);
  const focusTerminal = () => terminals.terminalFor(ref).term.focus();

  const startReview = () => {
    onReviewOpenChange(true);
    api.startReview(task.id).catch(onError);
  };
  const sendReview = () => {
    if (!task.review) return;
    const text = `${t("review.handoff", { agent: task.reviewer.name })}\n\n${task.review.output}`;
    api.sendText(ref, text, true).catch(onError);
    onReviewOpenChange(false);
    focusTerminal();
  };

  return (
    <div className="task">
      <header className="topbar" data-tauri-drag-region>
        <AgentIcon kind={agent.kind} title={agent.name} />
        <span className={`topbar-title ellipsis ${task.untitled ? "untitled" : ""}`} title={task.prompt} data-tauri-drag-region>
          {task.untitled ? t("task.untitled") : task.title}
        </span>
        <StatusIcon status={agent.status} />
        <span className="topbar-meta" data-tauri-drag-region title={t("task.basedOn", { base: task.base })}>
          <GitBranch size={12} strokeWidth={1.75} />
          {task.repo && `${task.repo} · `}
          {task.branch}
          {diff && diff.files > 0 && (
            <>
              {"  "}
              <span className="ins">+{diff.insertions}</span> <span className="del">−{diff.deletions}</span>
            </>
          )}
        </span>
        <span className="grow" data-tauri-drag-region />
        <ReviewButton
          task={task}
          open={reviewOpen}
          installed={reviewerInstalled}
          onStart={startReview}
          onToggle={() => onReviewOpenChange(!reviewOpen)}
        />
        <div className="more" onMouseDown={(e) => e.stopPropagation()}>
          <button className="icon-btn" title={t("common.more")} onClick={() => setMenu((m) => !m)}>
            <Ellipsis size={15} strokeWidth={1.75} />
          </button>
          {menu && (
            <MoreMenu
              onDismiss={() => setMenu(false)}
              items={[
                { label: t(isMac ? "task.reveal" : "task.openFolder"), kbd: "⌘⇧F", run: () => void api.reveal(task.worktree) },
                { label: t("task.openTerminal"), run: () => void api.openTerminal(task.worktree) },
                { label: t("task.copyPath"), run: () => void navigator.clipboard.writeText(task.worktree) },
                { sep: true },
                { label: t("task.close"), kbd: "⌘⇧W", run: onClose, danger: true },
              ]}
            />
          )}
        </div>
      </header>

      <div className="task-body">
        <div className="task-main">
          <AgentTerminal key={`${task.id}:${agent.id}`} task={task} agent={agent} onError={onError} />
          <VoiceBar agent={ref} voice={voice} />
        </div>
        {reviewOpen && (
          <ReviewPanel
            task={task}
            onRerun={startReview}
            onCancel={() => void api.cancelReview(task.id)}
            onSend={sendReview}
            onClose={() => {
              onReviewOpenChange(false);
              focusTerminal();
            }}
          />
        )}
      </div>
    </div>
  );
}

/** 顶部的审阅入口：平时是一个小按钮，审阅中显示用时，有结论时显示摘要。 */
function ReviewButton({
  task,
  open,
  installed,
  onStart,
  onToggle,
}: {
  task: Task;
  open: boolean;
  installed: boolean;
  onStart(): void;
  onToggle(): void;
}) {
  const t = useT();
  const now = useNow(task.reviewingSince != null);
  if (task.reviewingSince != null) {
    return (
      <button className={`review-btn is-running ${open ? "is-open" : ""}`} onClick={onToggle} title={t("review.runningTitle", { agent: task.reviewer.name })}>
        <span className="status status-running" />
        {t("review.running", { time: elapsed(task.reviewingSince, now) })}
      </button>
    );
  }
  if (task.review) {
    return (
      <button
        className={`review-btn has-result ${task.review.ok ? "" : "is-failed"} ${open ? "is-open" : ""}`}
        onClick={onToggle}
        title={t("review.resultTitle")}
      >
        <span className="review-dot" />
        <span className="ellipsis">{task.review.ok ? task.review.summary : t("review.failed")}</span>
      </button>
    );
  }
  return (
    <button
      className="review-btn"
      onClick={onStart}
      disabled={!installed}
      title={
        installed
          ? t("review.buttonTitle", { agent: task.reviewer.name })
          : t("review.unavailable", { agent: task.reviewer.name })
      }
    >
      <AgentIcon kind={task.reviewer.kind} size={13} />
      {t("review.button")}
    </button>
  );
}

function ReviewPanel({
  task,
  onRerun,
  onCancel,
  onSend,
  onClose,
}: {
  task: Task;
  onRerun(): void;
  onCancel(): void;
  onSend(): void;
  onClose(): void;
}) {
  const t = useT();
  const writerName = writerOf(task)?.name ?? "";
  const running = task.reviewingSince != null;
  const now = useNow(running);
  return (
    <aside className="review-panel">
      <div className="review-head">
        <span className="grow">
          {t("review.panelTitle", { agent: task.reviewer.name })}
          <span className="meta">
            {running
              ? ` · ${t("review.inProgress", { time: elapsed(task.reviewingSince!, now) })}`
              : task.review
                ? ` · ${t("common.ago", { time: elapsed(task.review.finishedAt, now) })}`
                : ""}
          </span>
        </span>
        <button className="icon-btn" title={t("common.collapse")} onClick={onClose}>
          <X size={14} strokeWidth={1.75} />
        </button>
      </div>
      <div className="review-body">
        {running && !task.review && (
          <p className="review-note">{t("review.runningNote", {
              branch: task.branch,
              base: task.base,
              reviewer: task.reviewer.name,
              writer: writerName,
            })}</p>
        )}
        {task.review && (
          <div className={`review-text ${task.review.ok ? "" : "is-failed"}`}>
            <Markdown text={task.review.output} />
          </div>
        )}
        {!running && !task.review && <p className="review-note">{t("review.none")}</p>}
      </div>
      <div className="review-foot">
        {running ? (
          <button className="btn" onClick={onCancel}>
            {t("review.stop")}
          </button>
        ) : (
          <>
            {task.review?.ok && (
              <button className="btn primary" onClick={onSend}>
                {t("review.sendToWriter", { agent: writerName })}
              </button>
            )}
            <button className="btn" onClick={onRerun}>
              {task.review ? t("review.rerun") : t("review.start")}
            </button>
          </>
        )}
      </div>
    </aside>
  );
}

/** Agent 的 CLI。没有运行时自动启动（第一次带需求，之后接着上次的会话）。 */
function AgentTerminal({ task, agent, onError }: { task: Task; agent: Agent; onError(err: unknown): void }) {
  const t = useT();
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
        <div className="terminal-note">{t("task.starting", { agent: agent.name })}</div>
      )}
      {ended && (
        <div className="terminal-ended">
          <span>
            {t(agent.status === "failed" ? "task.agentFailed" : "task.agentExited", { agent: agent.name })}
          </span>
          <button className="btn" onClick={() => void start()} disabled={starting}>
            {t("task.resume")}
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
            {item.kbd && <kbd>{kbd(item.kbd)}</kbd>}
          </button>
        ),
      )}
    </div>
  );
}

/** `active` 时每秒刷新一次的当前时间（秒）。 */
function useNow(active: boolean) {
  const [now, setNow] = useState(() => Date.now() / 1000);
  useEffect(() => {
    if (!active) return;
    setNow(Date.now() / 1000);
    const id = window.setInterval(() => setNow(Date.now() / 1000), 1000);
    return () => window.clearInterval(id);
  }, [active]);
  return now;
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
