import { useEffect, useRef, useState } from "react";

import { useT } from "../i18n";
import type { AgentOption } from "../lib/agents";
import type { Task } from "../lib/api";
import { writerOf } from "../lib/format";
import { useModalFocus } from "../lib/modal";
import { AgentIcon } from "./AgentIcon";

/** 交出去之后做什么：审阅、在这个任务里接力、分出新任务。 */
export type Handoff =
  | { kind: "review"; agent: string }
  | { kind: "relay"; agent: string; request: string }
  | { kind: "fork"; agent: string; request: string };

/**
 * 交给另一个 Agent：选谁来、做什么（审阅还是接着做）、接着做时在哪做（这个任务里接力，还是分出新任务两边并行）。
 * 三种都带上同一份交接说明（需求、最近的对话、现在的改动），由引擎生成。↵ 交出去，Esc 取消。
 */
export function HandoffDialog({
  task,
  agents,
  onRun,
  onDone,
  onError,
}: {
  task: Task;
  agents: AgentOption[];
  onRun(handoff: Handoff): Promise<void>;
  onDone(): void;
  onError(err: unknown): void;
}) {
  const t = useT();
  const writer = writerOf(task) ?? task.agents[0];
  const installed = agents.filter((a) => a.installed);
  // 默认交给上次叫来审阅的、不是现在在写的那个 Agent。
  const [agent, setAgent] = useState(
    () =>
      installed.find((a) => a.id === task.reviewer.kind && a.id !== writer.kind)?.id ??
      installed.find((a) => a.id !== writer.kind)?.id ??
      writer.kind,
  );
  const [what, setWhat] = useState<"review" | "continue">("continue");
  const [where, setWhere] = useState<"relay" | "fork">("fork");
  const [request, setRequest] = useState("");
  const [running, setRunning] = useState(false);
  const first = useRef<HTMLButtonElement>(null);
  useModalFocus(first);

  const picked = installed.find((a) => a.id === agent);
  const name = picked?.name ?? agent;
  const kind: Handoff["kind"] = what === "review" ? "review" : where;
  // 不能交的情况：审阅时这个 Agent 不能审；接力时就是它在写。
  const blocked =
    kind === "review" && !picked?.reviews
      ? t("handoff.cantReview", { agent: name })
      : kind === "relay" && agent === writer.kind
        ? t("handoff.alreadyWriting", { agent: name })
        : null;
  const foot =
    blocked ??
    (kind === "review"
      ? t("handoff.reviewFoot", { agent: name })
      : kind === "relay"
        ? t(task.resumable.includes(agent) ? "handoff.relayBackFoot" : "handoff.relayFoot", { from: writer.name, to: name })
        : t("handoff.forkFoot", { from: writer.name }));
  const action =
    kind === "review"
      ? t("handoff.startReview")
      : kind === "relay"
        ? t("handoff.relayAction", { agent: name })
        : t("handoff.forkAction");

  const run = async () => {
    if (blocked || running) return;
    setRunning(true);
    try {
      await onRun(kind === "review" ? { kind, agent } : { kind, agent, request: request.trim() });
    } catch (err) {
      onError(err);
    }
    onDone();
  };

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        onDone();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [onDone]);

  return (
    <div className="scrim" onMouseDown={onDone}>
      <div className="dialog handoff" role="dialog" aria-labelledby="handoff-title" onMouseDown={(e) => e.stopPropagation()}>
        <h2 id="handoff-title">{t("handoff.title")}</h2>

        <div className="handoff-field">
          <span className="handoff-label">{t("handoff.who")}</span>
          <div className="handoff-agents">
            {installed.map((a, i) => (
              <button
                key={a.id}
                ref={i === 0 ? first : undefined}
                className={`chip ${a.id === agent ? "is-open" : ""}`}
                aria-pressed={a.id === agent}
                onClick={() => setAgent(a.id)}
              >
                <AgentIcon kind={a.id} size={13} />
                {a.name}
              </button>
            ))}
          </div>
        </div>

        <div className="handoff-field">
          <span className="handoff-label">{t("handoff.what")}</span>
          <div className="handoff-choices">
            <Choice on={what === "continue"} title={t("handoff.continue")} note={t("handoff.continueNote")} onPick={() => setWhat("continue")} />
            <Choice on={what === "review"} title={t("handoff.review")} note={t("handoff.reviewNote")} onPick={() => setWhat("review")} />
          </div>
        </div>

        {what === "continue" && (
          <>
            <div className="handoff-field">
              <span className="handoff-label">{t("handoff.where")}</span>
              <div className="handoff-choices">
                <Choice
                  on={where === "fork"}
                  title={t("handoff.fork")}
                  note={t("handoff.forkNote", { agent: writer.name })}
                  onPick={() => setWhere("fork")}
                />
                <Choice
                  on={where === "relay"}
                  title={t("handoff.relay")}
                  note={t("handoff.relayNote", { agent: writer.name })}
                  onPick={() => setWhere("relay")}
                />
              </div>
            </div>
            <label className="handoff-field">
              <span className="handoff-label">{t("handoff.request")}</span>
              <input
                className="text-input"
                value={request}
                placeholder={t("handoff.requestPlaceholder")}
                onChange={(e) => setRequest(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter" && !e.nativeEvent.isComposing) void run();
                }}
              />
            </label>
            <p className="handoff-carries">{t("handoff.carries")}</p>
          </>
        )}

        <p className={`handoff-foot ${blocked ? "is-blocked" : ""}`}>{foot}</p>
        <div className="dialog-actions">
          <button className="btn" onClick={onDone}>
            {t("common.cancel")}
          </button>
          <button className="btn primary" disabled={running || blocked != null} onClick={() => void run()}>
            {running ? t("common.pleaseWait") : action}
          </button>
        </div>
      </div>
    </div>
  );
}

function Choice({ on, title, note, onPick }: { on: boolean; title: string; note: string; onPick(): void }) {
  return (
    <button className={`handoff-choice ${on ? "is-on" : ""}`} aria-pressed={on} onClick={onPick}>
      <span className="handoff-choice-title">{title}</span>
      <span className="handoff-choice-note">{note}</span>
    </button>
  );
}
