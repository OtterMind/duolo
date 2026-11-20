import { useEffect, useRef, useState } from "react";

import type { Voice } from "../lib/voice";

/** 录音中 / 识别中的一行状态。 */
export function VoiceStatus({ voice }: { voice: Voice }) {
  const [, tick] = useState(0);
  useEffect(() => {
    const id = window.setInterval(() => tick((n) => n + 1), 500);
    return () => window.clearInterval(id);
  }, []);

  if (voice.phase.kind === "recording") {
    const secs = Math.floor((Date.now() - voice.phase.since) / 1000);
    return (
      <span className="voice-status">
        <span className="rec-dot" /> 正在听 {Math.floor(secs / 60)}:{String(secs % 60).padStart(2, "0")}
        <span className="hint">
          <kbd>⌃Space</kbd> 结束 <kbd>Esc</kbd> 取消
        </span>
      </span>
    );
  }
  if (voice.phase.kind === "processing") {
    return (
      <span className="voice-status">
        <span className="status status-running" /> 正在整理…
      </span>
    );
  }
  return null;
}

/** 任务里的语音：浮在 Agent 输入框上方，确认后再发给 Agent。 */
export function VoiceOverlay({ voice, onDone }: { voice: Voice; onDone(): void }) {
  const input = useRef<HTMLTextAreaElement>(null);
  const phase = voice.phase;

  useEffect(() => {
    if (phase.kind === "review") {
      const el = input.current;
      el?.focus();
      el?.setSelectionRange(el.value.length, el.value.length);
    }
  }, [phase.kind]);

  useEffect(() => {
    const el = input.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${Math.min(el.scrollHeight, 200)}px`;
  });

  // 录音时 Esc 取消（焦点可能还在终端里）。
  useEffect(() => {
    if (phase.kind !== "recording" && phase.kind !== "processing") return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        voice.cancel();
        onDone();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [phase.kind, voice, onDone]);

  if (phase.kind === "idle") return null;

  return (
    <div className="voice-overlay">
      {phase.kind === "review" ? (
        <>
          <textarea
            ref={input}
            rows={1}
            value={phase.text}
            onChange={(e) => voice.edit(e.target.value)}
            onKeyDown={(e) => {
              if (e.nativeEvent.isComposing) return;
              if (e.key === "Enter" && !e.shiftKey) {
                e.preventDefault();
                voice.send(true);
                onDone();
              } else if (e.key === "Tab") {
                e.preventDefault();
                voice.send(false);
                onDone();
              } else if (e.key === "Escape") {
                e.preventDefault();
                if (!voice.revert()) {
                  voice.cancel();
                  onDone();
                }
              }
            }}
          />
          <div className="voice-foot">
            {phase.organized && phase.text !== phase.raw ? <span>已整理</span> : <span>原话</span>}
            <span className="grow" />
            <span className="hint">
              <kbd>↵</kbd> 发送 <kbd>Tab</kbd> 只填入 <kbd>Esc</kbd>{" "}
              {phase.organized && phase.text !== phase.raw ? "看原话" : "取消"}
            </span>
          </div>
        </>
      ) : (
        <div className="voice-foot">
          <VoiceStatus voice={voice} />
        </div>
      )}
    </div>
  );
}
