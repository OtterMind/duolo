// 语音输入：⌘⇧Space 或点输入行右侧的麦克风开始录音，再按一次结束。
// （不用 ⌃Space：那是 macOS 切换输入法的默认快捷键。）
// 识别和整理在后端完成，结果直接填进 Agent 自己的输入框（不发送），看一眼按 ↵ 发出去。

import { useCallback, useRef, useState } from "react";

import { api, type AgentRef } from "./api";
import type { Translate } from "../i18n";

export type VoicePhase = { kind: "idle" } | { kind: "recording"; since: number } | { kind: "processing" };

export interface Voice {
  phase: VoicePhase;
  ready: boolean;
  toggle(): void;
  cancel(): void;
}

interface Options {
  ready: boolean;
  target: AgentRef | null;
  context: { project?: string; task?: string };
  /** 文字已经填进了 Agent 的输入框。 */
  onInserted(target: AgentRef, text: string): void;
  onError(err: unknown): void;
  t: Translate;
}

export function useVoice(opts: Options): Voice {
  const [phase, setPhase] = useState<VoicePhase>({ kind: "idle" });
  const optsRef = useRef(opts);
  optsRef.current = opts;
  const phaseRef = useRef(phase);
  phaseRef.current = phase;
  /** 录音开始时的目标，结束时可能已经切到别的任务。 */
  const targetRef = useRef<AgentRef | null>(null);

  const toggle = useCallback(() => {
    const { ready, target, context, onError, onInserted, t } = optsRef.current;
    const current = phaseRef.current;
    if (current.kind === "idle") {
      if (!ready) return onError({ code: "voiceNotConfigured" });
      if (!target) return onError(t("voice.needTask"));
      targetRef.current = target;
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
      .then(async ({ raw, organized }) => {
        const to = targetRef.current;
        const text = (organized ?? raw).trim();
        if (to && text) {
          await api.sendText(to, text, false);
          onInserted(to, text);
        }
      })
      .catch(onError)
      .finally(() => setPhase({ kind: "idle" }));
  }, []);

  const cancel = useCallback(() => {
    if (phaseRef.current.kind === "recording") void api.voiceCancel();
    setPhase({ kind: "idle" });
  }, []);

  return { phase, ready: opts.ready, toggle, cancel };
}
