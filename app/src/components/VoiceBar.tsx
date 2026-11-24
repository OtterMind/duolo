import { useEffect, useState } from "react";
import { ArrowUp, Check, Lock, Mic, MicOff, Settings2, Trash2, Undo2, X } from "lucide-react";

import { useT } from "../i18n";
import { api, type AgentRef } from "../lib/api";
import * as terminals from "../lib/terminals";
import type { Voice } from "../lib/voice";
import { TALK_KEY } from "../lib/platform";

/** 终端区域的上内边距，和 .terminal-wrap 保持一致。 */
const PAD_TOP = 10;
const ICON = { size: 14, strokeWidth: 1.75 } as const;

/**
 * 语音的状态，都在 Agent 的输入块上：录音时输入块变成录音条
 * （听到的文字、波形、计时，右边是丢弃 / 插入 / 插入并发送三个图标）；插入后右上角出现换回原话和撤销。
 * 入口是终端下面的 `VoiceButton`。
 */
export function VoiceBar({ agent, voice }: { agent: AgentRef; voice: Voice }) {
  const t = useT();
  const block = useInputBlock(agent);
  if (!block) return null;
  const top = block.top + PAD_TOP;
  // 录音条从输入块底边往上长，文字多时不会盖住下面。
  const anchor = { bottom: `calc(100% - ${top + block.height}px)`, minHeight: block.height };
  const { phase, notice } = voice;
  const forAgent = voice.targetKind !== "text";

  if (notice === "permission") {
    return (
      <div className="voice-bar is-warn" style={anchor}>
        <MicOff {...ICON} className="voice-warn-icon" />
        <span className="grow">{t("voice.permission")}</span>
        <button className="btn primary small" onClick={() => void api.openSystemSettings("microphone")}>
          {t("voice.openSettings")}
        </button>
        <button className="icon-btn" title={t("voice.dismiss")} onClick={voice.dismissNotice}>
          <X {...ICON} />
        </button>
      </div>
    );
  }

  if (phase.kind === "recording" && forAgent) {
    return (
      <div className="voice-bar is-live" style={anchor}>
        <div className="voice-text">
          {notice === "silent" ? (
            <span className="voice-silent">
              {t("voice.silent")}
              <button
                className="icon-btn"
                title={t("voice.soundSettings")}
                onClick={() => void api.openSystemSettings("sound")}
              >
                <Settings2 {...ICON} />
              </button>
            </span>
          ) : (
            voice.heard || <span className="voice-placeholder">{t("voice.listening")}</span>
          )}
        </div>
        <div className="voice-controls">
          <span className="rec-dot" />
          <Waveform levels={voice.levels} silent={notice === "silent"} />
          <Elapsed since={phase.since} />
          {phase.locked && (
            <span className="voice-lock" title={t("voice.locked", { key: TALK_KEY })}>
              <Lock size={12} strokeWidth={1.75} />
            </span>
          )}
          <span className="grow" />
          <button className="icon-btn" title={t("voice.discard")} onClick={voice.cancel}>
            <X {...ICON} />
          </button>
          <button className="icon-btn strong" title={t("voice.insert")} onClick={() => voice.finish(false)}>
            <Check {...ICON} />
          </button>
          <button className="icon-btn solid" title={t("voice.send")} onClick={() => voice.finish(true)}>
            <ArrowUp size={14} strokeWidth={2.2} />
          </button>
        </div>
      </div>
    );
  }

  if (phase.kind === "processing" && forAgent) {
    return (
      <div className="voice-bar" style={anchor}>
        <span className="status status-running" />
        <span className="voice-placeholder">{t("voice.processing")}</span>
      </div>
    );
  }

  const ins = voice.inserted;
  if (!ins || ins.target.kind !== "agent") return null;
  return (
    <div className="voice-inserted" style={{ top: top - 34 }}>
      {ins.organized && (
        <button className="icon-btn" title={t("voice.revert")} onClick={voice.revert}>
          <Undo2 {...ICON} />
        </button>
      )}
      <button className="icon-btn" title={t("voice.undo")} onClick={voice.undo}>
        <Trash2 {...ICON} />
      </button>
    </div>
  );
}

/** 语音入口：终端下面一个写着"按住 Fn 说话"的按钮，比输入框末尾的小图标好找；点一下也能开始和结束录音。 */
export function VoiceButton({ voice }: { voice: Voice }) {
  const t = useT();
  const recording = voice.phase.kind === "recording";
  return (
    <div className="voice-strip">
      <button
        className={`voice-talk ${recording ? "is-recording" : ""} ${voice.ready ? "" : "is-off"}`}
        title={voice.ready ? t("voice.title", { key: TALK_KEY }) : t("voice.notConfiguredTitle")}
        onMouseDown={(e) => e.preventDefault()}
        onClick={voice.toggle}
      >
        {recording ? <span className="rec-dot" /> : <Mic {...ICON} />}
        {t("voice.hold", { key: TALK_KEY })}
      </button>
    </div>
  );
}

/** 音量波形：最近的音量从左到右排开。 */
export function Waveform({ levels, silent }: { levels: number[]; silent?: boolean }) {
  const bars = [...Array(Math.max(0, 16 - levels.length)).fill(0), ...levels.slice(-16)];
  return (
    <span className={`waveform ${silent ? "is-silent" : ""}`} aria-hidden>
      {bars.map((level, i) => (
        <span key={i} style={{ height: 2 + Math.round(level * 14) }} />
      ))}
    </span>
  );
}

function Elapsed({ since }: { since: number }) {
  const [, tick] = useState(0);
  useEffect(() => {
    const id = window.setInterval(() => tick((n) => n + 1), 500);
    return () => window.clearInterval(id);
  }, []);
  const secs = Math.floor((Date.now() - since) / 1000);
  return (
    <span className="voice-time">
      {Math.floor(secs / 60)}:{String(secs % 60).padStart(2, "0")}
    </span>
  );
}

/** Agent 输入块的位置，画面变化时更新。 */
function useInputBlock(agent: AgentRef) {
  const [pos, setPos] = useState<{ top: number; height: number } | null>(null);
  useEffect(() => {
    const update = () =>
      setPos((old) => {
        const next = terminals.inputBlock(agent);
        return old?.top === next?.top && old?.height === next?.height ? old : next;
      });
    update();
    return terminals.onInputBlockChange(agent, update);
  }, [agent.task, agent.agent]);
  return pos;
}
