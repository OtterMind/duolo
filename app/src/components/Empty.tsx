import type { Check, Snapshot } from "../lib/api";

interface Props {
  snap: Snapshot | null;
  env: Check[] | null;
  creating: boolean;
  onNewTask(): void;
  onAddProject(): void;
}

/** 右侧没有任务可显示时：还没有项目，或者任务都关掉了。 */
export function Empty({ snap, env, creating, onNewTask, onAddProject }: Props) {
  const noProject = snap !== null && snap.projects.length === 0;
  return (
    <div className="empty">
      <div className="tabbar" data-tauri-drag-region />
      <div className="empty-body" data-tauri-drag-region>
        {!snap || creating ? (
          <p className="empty-note">{creating ? "正在准备工作目录…" : ""}</p>
        ) : noProject ? (
          <>
            <h1>添加一个项目开始</h1>
            <p className="empty-note">
              选择一个 git 仓库。每个任务会在独立的分支和工作目录里进行，不会动到你手上的代码。
            </p>
            <button className="btn primary" onClick={onAddProject}>
              选择文件夹…
            </button>
          </>
        ) : (
          <>
            <p className="empty-note">没有打开的任务</p>
            <button className="btn" onClick={onNewTask}>
              新任务 <kbd>⌘N</kbd>
            </button>
          </>
        )}
        <EnvLine env={env} />
      </div>
    </div>
  );
}

function EnvLine({ env }: { env: Check[] | null }) {
  if (!env || env.length === 0) return null;
  const missing = env.filter((c) => !c.ok && c.name !== "语音");
  return (
    <p className="env">
      {env.map((c) => (
        <span key={c.name} className={c.ok ? "ok" : c.name === "语音" ? "" : "bad"} title={c.detail}>
          {c.ok ? "✓" : c.name === "语音" ? "·" : "✕"} {c.name}
          {c.ok && c.detail && c.detail !== "已配置" ? ` ${c.detail}` : ""}
        </span>
      ))}
      {missing.length > 0 && (
        <span className="bad block">{missing.map((c) => c.detail).join("，")}。安装后重新打开 Duolo。</span>
      )}
    </p>
  );
}
