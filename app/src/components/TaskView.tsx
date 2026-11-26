import { useCallback, useEffect, useRef, useState } from "react";
import { ChevronDown, Columns2, Ellipsis, GitBranch, MessagesSquare, PanelRightClose, SquareTerminal, X } from "lucide-react";

import { useT } from "../i18n";

import { api, type Agent, type Diff, type Surface, type Task } from "../lib/api";
import type { AgentOption } from "../lib/agents";
import { elapsed, isLive, writerOf } from "../lib/format";
import * as terminals from "../lib/terminals";
import type { Voice, VoiceTarget } from "../lib/voice";
import { ChatView } from "./ChatView";
import { StatusIcon } from "./StatusIcon";
import { AgentIcon } from "./AgentIcon";
import { AgentList, Popover } from "./AgentMenu";
import type { ConfirmSpec } from "./Confirm";
import { Markdown } from "./Markdown";
import { VoiceBar } from "./VoiceBar";
import { isMac, kbd } from "../lib/platform";

interface Props {
  task: Task;
  voice: Voice;
  /** 聊天模式下，语音文字插到对话的输入框里。 */
  setVoiceTarget(target: VoiceTarget | null): void;
  reviewOpen: boolean;
  /** 这个任务的审阅 Agent 本机装了没有。 */
  /** 可以换成的 Agent。 */
  agents: AgentOption[];
  onReviewOpenChange(open: boolean): void;
  /** 交给另一个 Agent：审阅、接力或分出新任务。 */
  onHandoff(): void;
  /** 没分屏时：在旁边再开一个任务（⌘\\）。 */
  onSplit?(): void;
  /** 分屏时：关掉这一格。 */
  onUnsplit?(): void;
  onConfirm(spec: ConfirmSpec): void;
  onClose(): void;
  onError(err: unknown): void;
}

/** 任务页：顶部一行任务信息和审阅入口，下面就是写代码的 Agent 自己的命令行。 */
export function TaskView(props: Props) {
  const { task, voice, reviewOpen, agents, onReviewOpenChange, onClose, onError } = props;
  const t = useT();
  const agent = writerOf(task) ?? task.agents[0];
  const chat = agent.surface === "chat";
  const ref = { task: task.id, agent: agent.id };
  const diff = useDiff(task.id);
  const [menu, setMenu] = useState(false);
  const focusTerminal = () => {
    if (!chat) terminals.terminalFor(ref).term.focus();
  };

  /** 叫 `reviewer`（不给就是上次叫的那个）来审阅。 */
  const startReview = (reviewer?: string) => {
    onReviewOpenChange(true);
    api.startReview(task.id, reviewer).catch(onError);
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
        <button className="agent-switch" title={t("task.writer", { agent: agent.name })} onClick={props.onHandoff}>
          <AgentIcon kind={agent.kind} />
          <span>{agent.name}</span>
          <ChevronDown size={12} strokeWidth={1.75} className="dim" />
        </button>
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
        {agent.chat && <SurfaceSwitch task={task} agent={agent} onConfirm={props.onConfirm} onError={onError} />}
        <ReviewButton task={task} open={reviewOpen} onHandoff={props.onHandoff} onToggle={() => onReviewOpenChange(!reviewOpen)} />
        {props.onSplit && (
          <button className="icon-btn" title={`${t("task.split")} (${kbd("⌘\\")})`} onClick={props.onSplit}>
            <Columns2 size={15} strokeWidth={1.75} />
          </button>
        )}
        {props.onUnsplit && (
          <button className="icon-btn" title={t("task.unsplit")} onClick={props.onUnsplit}>
            <PanelRightClose size={15} strokeWidth={1.75} />
          </button>
        )}
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
          {chat ? (
            <ChatView
              key={`${task.id}:${agent.id}`}
              task={task}
              agent={agent}
              voice={voice}
              setVoiceTarget={props.setVoiceTarget}
              onError={onError}
            />
          ) : (
            <>
              <AgentTerminal key={`${task.id}:${agent.id}`} task={task} agent={agent} onError={onError} />
              <VoiceBar agent={ref} voice={voice} />
            </>
          )}
        </div>
        {reviewOpen && (
          <ReviewPanel
            task={task}
            agents={agents}
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

/** 终端 / 聊天：同一个会话换个方式继续，Agent 会重新启动。 */
/** 在终端和聊天之间切换：换个方式重新启动，接着同一个会话。它正在干活时先确认（这一轮会被打断）。 */
function SurfaceSwitch({
  task,
  agent,
  onConfirm,
  onError,
}: {
  task: Task;
  agent: Agent;
  onConfirm(spec: ConfirmSpec): void;
  onError(err: unknown): void;
}) {
  const t = useT();
  const pick = (surface: Surface) => {
    if (surface === agent.surface) return;
    const busy = agent.status === "running" || agent.status === "needs_input";
    if (!busy) return void api.setSurface(task.id, surface).catch(onError);
    onConfirm({
      title: t(surface === "chat" ? "task.surfaceChatTitle" : "task.surfaceTerminalTitle"),
      body: t("task.surfaceBusyBody", { agent: agent.name }),
      action: t("task.switchWriterAction"),
      run: async () => {
        await api.setSurface(task.id, surface);
      },
    });
  };
  return (
    <div className="segmented" role="group">
      <button
        className={`icon-btn ${agent.surface === "terminal" ? "is-on" : ""}`}
        title={t("task.surfaceTerminal", { agent: agent.name })}
        onClick={() => pick("terminal")}
      >
        <SquareTerminal size={14} strokeWidth={1.75} />
      </button>
      <button
        className={`icon-btn ${agent.surface === "chat" ? "is-on" : ""}`}
        title={t("task.surfaceChat", { agent: agent.name })}
        onClick={() => pick("chat")}
      >
        <MessagesSquare size={14} strokeWidth={1.75} />
      </button>
    </div>
  );
}

/**
 * 顶部的「交给…」入口：平时点开交给另一个 Agent（审阅、接力或分出新任务）；审阅中显示用时，有结论时显示摘要，
 * 点了开关审阅面板。
 */
function ReviewButton({
  task,
  open,
  onHandoff,
  onToggle,
}: {
  task: Task;
  open: boolean;
  onHandoff(): void;
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
    <button className="review-btn" onClick={onHandoff} title={t("handoff.buttonTitle", { agent: task.reviewer.name })}>
      {t("handoff.button")}
    </button>
  );
}

function ReviewPanel({
  task,
  agents,
  onRerun,
  onCancel,
  onSend,
  onClose,
}: {
  task: Task;
  agents: AgentOption[];
  onRerun(reviewer: string): void;
  onCancel(): void;
  onSend(): void;
  onClose(): void;
}) {
  const t = useT();
  const writerName = writerOf(task)?.name ?? "";
  const running = task.reviewingSince != null;
  const now = useNow(running);
  const [picking, setPicking] = useState(false);
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
            <div className="dropdown-anchor">
              <button className="btn" onClick={() => setPicking((p) => !p)}>
                {task.review ? t("review.rerun") : t("review.start")}
              </button>
              {picking && (
                <ReviewerPicker
                  task={task}
                  agents={agents}
                  onPick={(id) => {
                    setPicking(false);
                    onRerun(id);
                  }}
                  onClose={() => setPicking(false)}
                />
              )}
            </div>
          </>
        )}
      </div>
    </aside>
  );
}

/** 选谁来审：能审阅的 Agent，上次叫的排在最前并打勾。 */
function ReviewerPicker({
  task,
  agents,
  onPick,
  onClose,
}: {
  task: Task;
  agents: AgentOption[];
  onPick(id: string): void;
  onClose(): void;
}) {
  const t = useT();
  const reviewers = agents.filter((a) => a.reviews);
  const ordered = [...reviewers.filter((a) => a.id === task.reviewer.kind), ...reviewers.filter((a) => a.id !== task.reviewer.kind)];
  return (
    <Popover onClose={onClose}>
      <AgentList title={t("newTask.reviewerTitle")} agents={ordered} selected={task.reviewer.kind} onPick={onPick} />
    </Popover>
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
        void api.resize(ref, size.rows, size.cols);
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
