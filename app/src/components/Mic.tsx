import { useEffect, useState } from "react";

import { Mic as MicGlyph } from "lucide-react";

import { useT } from "../i18n";
import { api, type AgentRef } from "../lib/api";
import * as terminals from "../lib/terminals";
import type { Voice } from "../lib/voice";

/** 终端的上内边距，和 .terminal-wrap 保持一致。 */
const PAD_TOP = 10;

/** 语音入口：贴在 Agent 输入行的最右边。录音和整理时在原位展开成一条状态。 */
export function Mic({ agent, voice }: { agent: AgentRef; voice: Voice }) {
  const t = useT();
  const top = useCursorTop(agent);
  const phase = voice.phase;
  const [, tick] = useState(0);
  const [heard, setHeard] = useState("");

  useEffect(() => {
    if (phase.kind !== "recording") return;
    setHeard("");
    // 实时显示目前听到的内容，同时刷新计时。
    const id = window.setInterval(() => {
      tick((n) => n + 1);
      api.voicePartial().then(setHeard, () => {});
    }, 300);
    // 录音时 Esc 取消（焦点在终端里也生效）。
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        voice.cancel();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => {
      window.clearInterval(id);
      window.removeEventListener("keydown", onKey, true);
    };
  }, [phase.kind, voice]);

  if (top === null) return null;
  const style = { top: top.top + PAD_TOP, height: top.height };

  if (phase.kind === "recording") {
    const secs = Math.floor((Date.now() - phase.since) / 1000);
    return (
      <div className="mic is-live" style={style}>
        <span className="rec-dot" />
        <span className="mic-time">
          {Math.floor(secs / 60)}:{String(secs % 60).padStart(2, "0")}
        </span>
        {heard && <span className="mic-heard">{heard}</span>}
        <span className="mic-hint">
          <kbd>⌘⇧Space</kbd> {t("voice.stop")} <kbd>Esc</kbd> {t("voice.cancel")}
        </span>
      </div>
    );
  }
  if (phase.kind === "processing") {
    return (
      <div className="mic is-live" style={style}>
        <span className="status status-running" />
        <span className="mic-hint">{t("voice.processing")}</span>
      </div>
    );
  }
  return (
    <button
      className={`mic ${voice.ready ? "" : "is-off"}`}
      style={style}
      title={voice.ready ? t("voice.title") : t("voice.notConfiguredTitle")}
      onMouseDown={(e) => e.preventDefault()}
      onClick={voice.toggle}
    >
      <MicGlyph size={14} strokeWidth={1.75} />
    </button>
  );
}

function useCursorTop(agent: AgentRef) {
  const [pos, setPos] = useState<{ top: number; height: number } | null>(null);
  useEffect(() => {
    const update = () => setPos(terminals.cursorRow(agent));
    update();
    return terminals.onCursorChange(agent, update);
  }, [agent.task, agent.agent]);
  return pos;
}
