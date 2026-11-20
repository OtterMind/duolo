import { useEffect, useRef, useState } from "react";

import type { Check, Snapshot } from "../lib/api";
import type { Voice } from "../lib/voice";
import { VoiceStatus } from "./VoiceBar";

interface Props {
  snap: Snapshot | null;
  env: Check[] | null;
  project: string | null;
  onProjectChange(id: string): void;
  onAddProject(): void;
  text: string;
  onTextChange(text: string): void;
  onSubmit(prompt: string): void;
  busy: boolean;
  voice: Voice;
}

/** 首页：说出或写下需求，↵ 新建任务并启动 Agent。 */
export function Home({ snap, env, project, onProjectChange, onAddProject, text, onTextChange, onSubmit, busy, voice }: Props) {
  const input = useRef<HTMLTextAreaElement>(null);
  const projects = snap?.projects ?? [];
  const current = projects.find((p) => p.id === project);
  const noProject = snap !== null && projects.length === 0;

  useEffect(() => {
    input.current?.focus();
  }, [project]);

  // 输入框随内容长高，最多 10 行。
  useEffect(() => {
    const el = input.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${Math.min(el.scrollHeight, 220)}px`;
  }, [text]);

  const submit = () => {
    if (!busy && text.trim()) onSubmit(text.trim());
  };

  return (
    <div className="home">
      <div className="titlebar" data-tauri-drag-region />
      <div className="home-hero" data-tauri-drag-region>
        <h1>{noProject ? "先添加一个项目" : "今天让 Agent 做点什么？"}</h1>
        {noProject ? (
          <p className="home-sub">
            选择一个 git 仓库。每个任务会在独立的分支和工作目录里进行，不会动到你手上的代码。
            <br />
            <button className="btn primary home-add" onClick={onAddProject}>
              选择文件夹…
            </button>
          </p>
        ) : (
          <p className="home-sub">Claude 写代码，写完可以让 Codex 审阅。每个任务一个独立分支。</p>
        )}
        <EnvLine env={env} />
      </div>

      <div className="composer-wrap">
        <div className={`composer ${busy ? "is-busy" : ""}`} onClick={() => input.current?.focus()}>
          <textarea
            ref={input}
            rows={1}
            value={text}
            placeholder={noProject ? "添加项目后，在这里描述需求" : "描述需求，比如：订单列表支持导出 CSV"}
            disabled={busy}
            onChange={(e) => onTextChange(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
                e.preventDefault();
                submit();
              } else if (e.key === "Escape" && voice.phase.kind !== "idle") {
                e.preventDefault();
                voice.cancel();
              }
            }}
          />
          <div className="composer-foot">
            <ProjectPicker
              snap={snap}
              current={current?.name}
              currentId={project}
              onPick={onProjectChange}
              onAdd={onAddProject}
            />
            <span className="grow" />
            {voice.phase.kind === "idle" || voice.phase.kind === "review" ? (
              <span className="hint">
                {busy ? (
                  "正在创建工作目录…"
                ) : (
                  <>
                    <kbd>↵</kbd> 开始 <kbd>⇧↵</kbd> 换行 <kbd>⌃Space</kbd> 语音
                  </>
                )}
              </span>
            ) : (
              <VoiceStatus voice={voice} />
            )}
          </div>
        </div>
      </div>
    </div>
  );
}

function EnvLine({ env }: { env: Check[] | null }) {
  if (!env) return <p className="env">正在检查环境…</p>;
  if (env.length === 0) return null;
  const agentsMissing = env.filter((c) => !c.ok && c.name !== "语音");
  return (
    <p className="env">
      {env.map((c) => (
        <span key={c.name} className={c.ok ? "ok" : c.name === "语音" ? "" : "bad"} title={c.detail}>
          {c.ok ? "✓" : c.name === "语音" ? "·" : "✕"} {c.name}
          {c.ok && c.detail && c.detail !== "已配置" ? ` ${c.detail}` : ""}
        </span>
      ))}
      {agentsMissing.length > 0 && (
        <span className="bad block">
          {agentsMissing.map((c) => c.detail).join("，")}。安装后重新打开 Duolo。
        </span>
      )}
    </p>
  );
}

function ProjectPicker({
  snap,
  current,
  currentId,
  onPick,
  onAdd,
}: {
  snap: Snapshot | null;
  current?: string;
  currentId: string | null;
  onPick(id: string): void;
  onAdd(): void;
}) {
  const [open, setOpen] = useState(false);
  useEffect(() => {
    if (!open) return;
    const close = () => setOpen(false);
    window.addEventListener("mousedown", close);
    return () => window.removeEventListener("mousedown", close);
  }, [open]);

  return (
    <div className="picker" onMouseDown={(e) => e.stopPropagation()}>
      <button
        className="picker-btn"
        onClick={(e) => {
          e.stopPropagation();
          if (!snap?.projects.length) onAdd();
          else setOpen((o) => !o);
        }}
      >
        <span className="picker-icon">▢</span>
        {current ?? "选择项目"}
        <span className="caret">▾</span>
      </button>
      {open && (
        <div className="menu picker-menu">
          {snap?.projects.map((p) => (
            <button
              key={p.id}
              className={`menu-item ${p.id === currentId ? "is-checked" : ""}`}
              onClick={() => {
                onPick(p.id);
                setOpen(false);
              }}
            >
              <span className="check">{p.id === currentId ? "✓" : ""}</span>
              <span className="grow">{p.name}</span>
              <span className="meta ellipsis">{p.root.replace(/^\/Users\/[^/]+/, "~")}</span>
            </button>
          ))}
          <div className="menu-sep" />
          <button
            className="menu-item"
            onClick={() => {
              setOpen(false);
              onAdd();
            }}
          >
            <span className="check" />
            <span className="grow">添加项目…</span>
            <kbd>⌘O</kbd>
          </button>
        </div>
      )}
    </div>
  );
}
