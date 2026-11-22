// 语音输入。
//
// - 按住 Fn 说话，松开把文字插入输入框（不发送）；按住 Fn 再点空格锁定，松手继续录，再按一次 Fn 结束。
//   Windows / Linux 上是按住 Ctrl+Shift+Space；轻按一下开始（锁定），再按一下结束。
// - 也可以点输入块右侧的麦克风，或用菜单里的 ⌘⇧Space：开始后再按一次结束。
// - 录音中：Esc 丢弃，↵ 插入，⌘↵（Ctrl+Enter）插入并发送。
// 按键的按下和松开由后端的系统级按键监听发来（`fn-key` 事件），Duolo 在后台时也有效。
// 识别和整理在后端完成；文字插入当前任务 Agent 的输入框，或者新建任务面板的输入框。

import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";

import { api, type AgentRef } from "./api";
import { isMac, modKey } from "./platform";

/** 文字插到哪里。 */
export type VoiceTarget =
  | { kind: "agent"; ref: AgentRef }
  /** 普通输入框（新建任务面板）：插入，以及把刚插入的文字换成另一段。 */
  | { kind: "text"; insert(text: string): void; replace(old: string, next: string): void };

export type VoicePhase =
  | { kind: "idle" }
  | { kind: "recording"; since: number; locked: boolean }
  | { kind: "processing" };

/** 输入块里显示的提示。 */
export type VoiceNotice = "permission" | "silent";

/** 刚插入的一段文字，可以换回原话或撤销。 */
export interface Inserted {
  target: VoiceTarget;
  text: string;
  raw: string;
  /** 整理过时为 true，可以换回原话。 */
  organized: boolean;
}

export interface Voice {
  phase: VoicePhase;
  /** 正在录的这段文字要插到哪种地方。 */
  targetKind: VoiceTarget["kind"] | null;
  ready: boolean;
  /** 录音中目前听到的内容。 */
  heard: string;
  /** 最近的音量（0~1），用来画波形。 */
  levels: number[];
  notice: VoiceNotice | null;
  inserted: Inserted | null;
  /** 点麦克风或 ⌘⇧Space：没在录就开始（锁定），在录就结束。 */
  toggle(): void;
  /** 结束录音并插入；`submit` 为真时顺便发送。 */
  finish(submit: boolean): void;
  cancel(): void;
  dismissNotice(): void;
  /** 把刚插入的整理结果换回原话。 */
  revert(): void;
  /** 删掉刚插入的文字。 */
  undo(): void;
}

interface Options {
  ready: boolean;
  /** 开始录音时调用，决定文字插到哪里。没有可插入的地方时返回 null。 */
  target(): VoiceTarget | null;
  context: { project?: string; task?: string };
  /** 文字已经插入 Agent 的输入框（还没发送）。 */
  onInserted(ref: AgentRef, text: string): void;
  onError(err: unknown): void;
  /** 没有可以插入的地方（没有打开任务）。 */
  onNoTarget(): void;
}

/** 按住 Fn 超过这么久才开始录音，避免 Fn+方向键这类组合键误触。 */
const HOLD_DELAY_MS = 250;
/** 录音状态和音量的刷新间隔。 */
const POLL_MS = 100;
/** 开始后这么久一直没有声音，就提示检查麦克风。 */
const SILENCE_MS = 3000;
/** 低于这个音量算没有声音。 */
const SILENCE_LEVEL = 0.12;
/** 波形保留的音量个数。 */
const LEVELS = 24;
/** 插入提示显示多久。 */
const INSERTED_MS = 5000;

export function useVoice(opts: Options): Voice {
  const [phase, setPhase] = useState<VoicePhase>({ kind: "idle" });
  const [heard, setHeard] = useState("");
  const [levels, setLevels] = useState<number[]>([]);
  const [notice, setNotice] = useState<VoiceNotice | null>(null);
  const [inserted, setInserted] = useState<Inserted | null>(null);
  const optsRef = useRef(opts);
  optsRef.current = opts;
  const phaseRef = useRef(phase);
  phaseRef.current = phase;
  const targetRef = useRef<VoiceTarget | null>(null);
  const [targetKind, setTargetKind] = useState<VoiceTarget["kind"] | null>(null);
  const insertedRef = useRef(inserted);
  insertedRef.current = inserted;

  const start = useCallback(async (locked: boolean) => {
    const { ready, target, onError, onNoTarget } = optsRef.current;
    if (phaseRef.current.kind !== "idle") return;
    if (!ready) return onError({ code: "voiceNotConfigured" });
    const to = target();
    if (!to) return onNoTarget();
    targetRef.current = to;
    setTargetKind(to.kind);
    setHeard("");
    setLevels([]);
    setNotice(null);
    setInserted(null);
    const next: VoicePhase = { kind: "recording", since: Date.now(), locked };
    phaseRef.current = next;
    setPhase(next);
    try {
      await api.voiceStart();
    } catch (err) {
      phaseRef.current = { kind: "idle" };
      setPhase({ kind: "idle" });
      if (isPermission(err)) setNotice("permission");
      else onError(err);
    }
  }, []);

  const cancel = useCallback(() => {
    if (phaseRef.current.kind === "recording") void api.voiceCancel();
    phaseRef.current = { kind: "idle" };
    setPhase({ kind: "idle" });
    setNotice((n) => (n === "silent" ? null : n));
  }, []);

  const finish = useCallback((submit: boolean) => {
    if (phaseRef.current.kind !== "recording") return;
    const { context, onError, onInserted } = optsRef.current;
    phaseRef.current = { kind: "processing" };
    setPhase({ kind: "processing" });
    setNotice((n) => (n === "silent" ? null : n));
    api
      .voiceFinish(context.project, context.task)
      .then(async ({ raw, organized }) => {
        const to = targetRef.current;
        const text = (organized ?? raw).trim();
        if (!to || !text) return;
        if (to.kind === "agent") {
          await api.sendText(to.ref, text, submit);
          onInserted(to.ref, text);
        } else {
          to.insert(text);
        }
        if (!submit) setInserted({ target: to, text, raw: raw.trim(), organized: organized != null });
      })
      .catch((err) => (isPermission(err) ? setNotice("permission") : onError(err)))
      .finally(() => {
        phaseRef.current = { kind: "idle" };
        setPhase({ kind: "idle" });
      });
  }, []);

  const toggle = useCallback(() => {
    const p = phaseRef.current;
    if (p.kind === "idle") void start(true);
    else if (p.kind === "recording") finish(false);
  }, [start, finish]);

  // 把刚插入的文字换成另一段（空字符串就是删掉）。
  const replaceInserted = useCallback((next: string) => {
    const ins = insertedRef.current;
    if (!ins) return;
    if (ins.target.kind === "agent") {
      const erase = "\x7f".repeat([...ins.text].length);
      void api.write(ins.target.ref, erase + next).catch(optsRef.current.onError);
    } else {
      ins.target.replace(ins.text, next);
    }
    setInserted(next ? { ...ins, text: next, organized: false } : null);
  }, []);
  const revert = useCallback(() => {
    const ins = insertedRef.current;
    if (ins?.organized) replaceInserted(ins.raw);
  }, [replaceInserted]);
  const undo = useCallback(() => replaceInserted(""), [replaceInserted]);

  // 录音中：刷新听到的内容和音量，一直没有声音时提示。
  useEffect(() => {
    if (phase.kind !== "recording") return;
    const since = phase.since;
    let loud = false;
    const id = window.setInterval(() => {
      api.voiceStatus().then(({ text, level }) => {
        setHeard(text);
        if (level == null) return;
        if (level >= SILENCE_LEVEL || text) loud = true;
        setLevels((l) => [...l.slice(-(LEVELS - 1)), level]);
        if (!loud && Date.now() - since > SILENCE_MS) setNotice("silent");
        else if (loud) setNotice((n) => (n === "silent" ? null : n));
      }, () => {});
    }, POLL_MS);
    return () => window.clearInterval(id);
  }, [phase]);

  // 录音中的按键：Esc 丢弃，↵ 插入，⌘↵ 插入并发送。焦点在终端里也生效。
  useEffect(() => {
    if (phase.kind !== "recording") return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") cancel();
      else if (e.key === "Enter") finish(modKey(e));
      else return;
      e.preventDefault();
      e.stopPropagation();
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [phase.kind, cancel, finish]);

  // 插入提示过一会儿自动消失；期间 ⌘Z 撤销。
  useEffect(() => {
    if (!inserted) return;
    const id = window.setTimeout(() => setInserted(null), INSERTED_MS);
    const onKey = (e: KeyboardEvent) => {
      if (modKey(e) && !e.shiftKey && e.key.toLowerCase() === "z" && inserted.target.kind === "agent") {
        e.preventDefault();
        e.stopPropagation();
        undo();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => {
      window.clearTimeout(id);
      window.removeEventListener("keydown", onKey, true);
    };
  }, [inserted, undo]);

  // 按住 Fn 说话。
  useEffect(() => {
    let held = false;
    let chord = false;
    let timer = 0;
    let downAt = 0;
    const unlisten = listen<string>("fn-key", ({ payload }) => {
      const p = phaseRef.current;
      switch (payload) {
        case "down":
          held = true;
          chord = false;
          downAt = Date.now();
          if (p.kind === "recording" && p.locked) {
            finish(false);
            held = false;
          } else if (p.kind === "idle") {
            timer = window.setTimeout(() => held && !chord && void start(false), HOLD_DELAY_MS);
          }
          break;
        case "space":
          if (held && p.kind === "recording" && !p.locked) {
            const next = { ...p, locked: true };
            phaseRef.current = next;
            setPhase(next);
          }
          break;
        case "chord":
          chord = true;
          break;
        case "up":
          held = false;
          window.clearTimeout(timer);
          if (p.kind === "recording" && !p.locked) finish(false);
          // Windows / Linux 上轻按一下快捷键：开始录音并锁定（macOS 上轻按 Fn 是切换输入法，不处理）。
          else if (!isMac && p.kind === "idle" && Date.now() - downAt < HOLD_DELAY_MS) toggle();
          break;
      }
    });
    return () => {
      window.clearTimeout(timer);
      void unlisten.then((f) => f());
    };
  }, [start, finish, toggle]);

  const dismissNotice = useCallback(() => setNotice(null), []);

  return {
    phase,
    targetKind,
    ready: opts.ready,
    heard,
    levels,
    notice,
    inserted,
    toggle,
    finish,
    cancel,
    dismissNotice,
    revert,
    undo,
  };
}

const isPermission = (err: unknown) =>
  typeof err === "object" && err !== null && (err as { code?: string }).code === "voicePermission";
