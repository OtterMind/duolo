import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { ArrowUp, FilePen, FileText, Globe, Mic, Search, Square, SquareTerminal, Trash2, Wrench } from "lucide-react";

import { useT } from "../i18n";
import { api, type Agent, type Task, type TaskEvent } from "../lib/api";
import { TALK_KEY } from "../lib/platform";
import type { Voice, VoiceTarget } from "../lib/voice";
import { Markdown } from "./Markdown";

interface Props {
  task: Task;
  agent: Agent;
  voice: Voice;
  /** 聊天页打开期间，语音文字插到输入框里。 */
  setVoiceTarget(target: VoiceTarget | null): void;
  onError(err: unknown): void;
}

/** 时间线上的一条：说的话、用的工具、问过的问题。 */
type Item = Extract<TaskEvent, { type: "message" | "tool" | "prompt" }>;

/**
 * 聊天模式下的任务页：对话由 Duolo 自己画（事件流），下面是输入框。和终端里是同一个会话，可以随时切回去。
 * 正在写的话、在用的工具来自快照（`agent.draft`），写完后变成事件。
 */
export function ChatView({ task, agent, voice, setVoiceTarget, onError }: Props) {
  const t = useT();
  const items = useTimeline(task.id);
  const [text, setText] = useState("");
  const textRef = useRef(text);
  textRef.current = text;
  const input = useRef<HTMLTextAreaElement>(null);
  const scroller = useRef<HTMLDivElement>(null);
  const stick = useRef(true);
  const [starting, setStarting] = useState(false);
  const ref = { task: task.id, agent: agent.id };
  const running = agent.status === "running";
  const ended = agent.status === "exited" || agent.status === "failed";

  const start = useCallback(async () => {
    setStarting(true);
    try {
      await api.startAgent(ref);
    } catch (err) {
      onError(err);
    } finally {
      setStarting(false);
    }
  }, [task.id, agent.id, onError]);

  // 没有进程就启动（第一次带需求，之后接着上次的会话）。
  useEffect(() => {
    if (agent.status === "not_started" && !starting) void start();
  }, [agent.status, task.id, agent.id]);

  // 语音文字插到光标处；"插入并发送"时连同输入框里已有的一起发出去。
  useEffect(() => {
    setVoiceTarget({
      kind: "text",
      insert: (said, submit) => {
        const el = input.current;
        const at = el?.selectionStart ?? textRef.current.length;
        const before = textRef.current.slice(0, at);
        const glue = before && !/\s$/.test(before) ? " " : "";
        const next = before + glue + said + textRef.current.slice(at);
        if (submit) void sendText(next);
        else setText(next);
        el?.focus();
      },
      replace: (old, next) => {
        const i = textRef.current.lastIndexOf(old);
        if (i >= 0) setText(textRef.current.slice(0, i) + next + textRef.current.slice(i + old.length));
      },
    });
    return () => setVoiceTarget(null);
  }, [setVoiceTarget]);

  useEffect(() => input.current?.focus(), [task.id]);

  // 在底部时跟着新内容往下滚；用户往上翻了就不动。
  useLayoutEffect(() => {
    const el = scroller.current;
    if (el && stick.current) el.scrollTop = el.scrollHeight;
  }, [items, agent.draft]);
  const onScroll = () => {
    const el = scroller.current;
    if (el) stick.current = el.scrollHeight - el.scrollTop - el.clientHeight < 40;
  };

  // 输入框跟着内容长高，最多 8 行左右。
  useLayoutEffect(() => {
    const el = input.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${Math.min(el.scrollHeight, 180)}px`;
  }, [text]);

  const run = async (command: Parameters<typeof api.execute>[0]) => {
    try {
      const result = await api.execute(command);
      if (!result.ok) onError({ code: result.error ?? "internal" });
      return result.ok;
    } catch (err) {
      onError(err);
      return false;
    }
  };

  const sendText = async (value: string) => {
    const said = value.trim();
    if (!said) return;
    stick.current = true;
    setText("");
    if (!(await run({ type: "send_message", task: task.id, text: said }))) setText(said);
  };
  const send = () => sendText(text);
  const stop = () => void run({ type: "interrupt", task: task.id });

  const onKey = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.nativeEvent.isComposing) return;
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      void send();
    } else if (e.key === "Escape" && running) {
      e.preventDefault();
      stop();
    }
  };

  // 还在等回答的问题：最后一个问题，而且 Agent 正在等。
  const lastPrompt = [...items].reverse().find((i) => i.type === "prompt");
  const asking = agent.status === "needs_input" && lastPrompt?.type === "prompt" ? lastPrompt : null;
  const draft = agent.draft;
  const recording = voice.phase.kind === "recording" && voice.targetKind === "text";

  return (
    <div className="chat">
      <div className="chat-scroll" ref={scroller} onScroll={onScroll}>
        <div className="chat-column">
          {items.length === 0 && !draft?.text && (
            <p className="chat-empty">
              {starting || agent.status === "not_started" ? t("task.starting", { agent: agent.name }) : t("chat.empty", { agent: agent.name })}
            </p>
          )}
          {items.map((item) => (
            <ChatItem key={item.seq} item={item} open={item === asking} />
          ))}
          {asking && (
            <div className="chat-ask">
              <div className="chat-ask-title">{t("chat.asks", { agent: agent.name })}</div>
              <div className="chat-ask-what">
                <ToolIcon tool={asking.question} />
                <span className="chat-tool-name">{asking.question}</span>
                {asking.detail && <code>{asking.detail}</code>}
              </div>
              <div className="chat-ask-options">
                {asking.options.map((option, i) => (
                  <button
                    key={i}
                    className={`btn ${i === 0 ? "primary" : ""}`}
                    onClick={() => void run({ type: "answer", task: task.id, option: i, prompt: asking.seq })}
                  >
                    {option}
                  </button>
                ))}
              </div>
            </div>
          )}
          {draft?.text && (
            <div className="chat-agent is-draft">
              <Markdown text={draft.text} />
            </div>
          )}
          {running && !draft?.text && (
            <div className="chat-working">
              <span className="status status-running" />
              {draft?.tool ? (
                <>
                  <ToolIcon tool={draft.tool} />
                  <span className="chat-tool-name">{draft.tool}</span>
                  {draft.target && <span className="chat-tool-target">{draft.target}</span>}
                </>
              ) : (
                <span>{draft?.thinking ? t("chat.thinking") : t("chat.working")}</span>
              )}
            </div>
          )}
          {agent.error && (
            <div className="chat-error">
              <div className="chat-error-title">{t("chat.agentError", { agent: agent.name })}</div>
              <div className="chat-error-detail">{agent.error}</div>
            </div>
          )}
        </div>
      </div>

      <div className="chat-composer">
        {ended && (
          <div className="chat-ended">
            <span>{t(agent.status === "failed" ? "task.agentFailed" : "task.agentExited", { agent: agent.name })}</span>
            <button className="btn" onClick={() => void start()} disabled={starting}>
              {t("task.resume")}
            </button>
          </div>
        )}
        <div className={`chat-input ${recording ? "is-recording" : ""}`} data-focus-home>
          <textarea
            ref={input}
            rows={1}
            value={recording ? voice.heard : text}
            placeholder={recording ? t("voice.listening") : t("chat.placeholder", { agent: agent.name })}
            onChange={(e) => setText(e.target.value)}
            onKeyDown={onKey}
            readOnly={recording}
          />
          {voice.ready && (
            <button className={`icon-btn ${recording ? "is-on" : ""}`} title={t("voice.title", { key: TALK_KEY })} onClick={voice.toggle}>
              <Mic size={15} strokeWidth={1.75} />
            </button>
          )}
          {running && !text.trim() ? (
            <button className="icon-btn chat-send" title={`${t("chat.stop")} (Esc)`} onClick={stop}>
              <Square size={12} strokeWidth={2.5} fill="currentColor" />
            </button>
          ) : (
            <button className="icon-btn chat-send" title={`${t("chat.send")} (↵)`} disabled={!text.trim()} onClick={() => void send()}>
              <ArrowUp size={15} strokeWidth={2} />
            </button>
          )}
        </div>
      </div>
    </div>
  );
}

function ChatItem({ item, open }: { item: Item; open: boolean }) {
  if (item.type === "message") {
    return item.speaker === "user" ? (
      <div className="chat-user">{item.text}</div>
    ) : (
      <div className="chat-agent">
        <Markdown text={item.text} />
      </div>
    );
  }
  // 回答过的问题和用过的工具：一行带过。正在问的在下面单独画。
  if (item.type === "prompt" && open) return null;
  const tool = item.type === "tool" ? item.tool : item.question;
  const target = item.type === "tool" ? item.target : item.detail;
  return (
    <div className="chat-tool">
      <ToolIcon tool={tool} />
      <span className="chat-tool-name">{tool}</span>
      {target && <span className="chat-tool-target">{target}</span>}
    </div>
  );
}

function ToolIcon({ tool }: { tool: string }) {
  const props = { size: 13, strokeWidth: 1.75, className: "chat-tool-icon" };
  const name = tool.toLowerCase();
  if (name === "read") return <FileText {...props} />;
  if (name === "edit" || name === "write" || name === "move") return <FilePen {...props} />;
  if (name === "bash" || name === "shell") return <SquareTerminal {...props} />;
  if (name === "search" || name === "grep" || name === "glob") return <Search {...props} />;
  if (name === "fetch" || name === "websearch" || name === "webfetch") return <Globe {...props} />;
  if (name === "delete") return <Trash2 {...props} />;
  return <Wrench {...props} />;
}

/** 任务的对话：先取已有的事件，之后跟着后端推送的新事件。 */
function useTimeline(task: number) {
  const [items, setItems] = useState<Item[]>([]);
  useEffect(() => {
    let alive = true;
    let last = 0;
    let buffered: TaskEvent[] = [];
    let loaded = false;
    const keep = (events: TaskEvent[]) => {
      const fresh = events.filter((e) => e.task === task && e.seq > last);
      if (fresh.length === 0) return;
      last = fresh[fresh.length - 1].seq;
      const shown = fresh.filter((e): e is Item => e.type === "message" || e.type === "tool" || e.type === "prompt");
      if (shown.length) setItems((old) => [...old, ...shown]);
    };
    setItems([]);
    // 取已有事件期间推来的先攒着，取完再接上，不丢不重。
    const unlisten = listen<TaskEvent[]>("task-events", (e) => (loaded ? keep(e.payload) : buffered.push(...e.payload)));
    api.taskEvents(task, 0).then(
      (events) => {
        if (!alive) return;
        keep(events);
        loaded = true;
        keep(buffered);
        buffered = [];
      },
      () => (loaded = true),
    );
    return () => {
      alive = false;
      void unlisten.then((stop) => stop());
    };
  }, [task]);
  return items;
}
