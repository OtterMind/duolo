// 语音输入：⌃Space 开始录音，再按一次结束；识别和整理在后端完成。
// 首页直接填进输入框；任务里先显示在 Agent 输入框上方，确认后再发送。

import { useCallback, useRef, useState } from "react";

import { api, type AgentRef } from "./api";

export type VoicePhase =
  | { kind: "idle" }
  | { kind: "recording"; since: number }
  | { kind: "processing" }
  | { kind: "review"; target: AgentRef; raw: string; organized: string | null; text: string };

export interface Voice {
  phase: VoicePhase;
  ready: boolean;
  toggle(): void;
  cancel(): void;
  edit(text: string): void;
  /** 发送给 Agent；`submit` 为假时只填进 Agent 的输入框。 */
  send(submit: boolean): void;
  /** 退回原话；已经是原话时返回 false。 */
  revert(): boolean;
}

interface Options {
  ready: boolean;
  target: AgentRef | null;
  context: { project?: string; task?: string };
  onHomeText(text: string): void;
  onError(err: unknown): void;
}

const NOT_READY =
  "语音还没配置：在 ~/.duolo/config.toml 里设置 voice.command（见 README）。也可以先用 macOS 听写，连按两下 Fn。";

export function useVoice(opts: Options): Voice {
  const [phase, setPhase] = useState<VoicePhase>({ kind: "idle" });
  const optsRef = useRef(opts);
  optsRef.current = opts;
  const phaseRef = useRef(phase);
  phaseRef.current = phase;

  const toggle = useCallback(() => {
    const { ready, target, context, onHomeText, onError } = optsRef.current;
    const current = phaseRef.current;
    if (current.kind === "idle" || current.kind === "review") {
      if (!ready) return onError(NOT_READY);
      api
        .voiceStart()
        .then(() => setPhase({ kind: "recording", since: Date.now() }))
        .catch(onError);
      return;
    }
    if (current.kind !== "recording") return;
    setPhase({ kind: "processing" });
    api
      .voiceFinish(context.project, context.task)
      .then(({ raw, organized }) => {
        const text = organized ?? raw;
        if (target) setPhase({ kind: "review", target, raw, organized, text });
        else {
          onHomeText(text);
          setPhase({ kind: "idle" });
        }
      })
      .catch((err) => {
        setPhase({ kind: "idle" });
        onError(err);
      });
  }, []);

  const cancel = useCallback(() => {
    if (phaseRef.current.kind === "recording") void api.voiceCancel();
    setPhase({ kind: "idle" });
  }, []);

  const edit = useCallback((text: string) => {
    setPhase((p) => (p.kind === "review" ? { ...p, text } : p));
  }, []);

  const send = useCallback((submit: boolean) => {
    const p = phaseRef.current;
    if (p.kind !== "review" || !p.text.trim()) return;
    api.sendText(p.target, p.text.trim(), submit).catch(optsRef.current.onError);
    setPhase({ kind: "idle" });
  }, []);

  const revert = useCallback(() => {
    const p = phaseRef.current;
    if (p.kind !== "review" || p.text === p.raw) return false;
    setPhase({ ...p, text: p.raw });
    return true;
  }, []);

  return { phase, ready: opts.ready, toggle, cancel, edit, send, revert };
}
