import { useEffect, useRef, useState } from "react";
import { Check, Copy, Download, LogIn } from "lucide-react";

import { useT } from "../i18n";
import { canSetUp, type AgentOption } from "../lib/agents";
import { requestSetup } from "../lib/setup";
import { AgentIcon } from "./AgentIcon";

/** 选谁写或谁审：一个下拉框，里面一组 Agent。 */
export function AgentMenu({
  onClose,
  ...list
}: {
  title: string;
  agents: AgentOption[];
  selected: string;
  onPick(id: string): void;
  onClose(): void;
}) {
  return (
    <Popover onClose={onClose}>
      <AgentList {...list} />
    </Popover>
  );
}

/** 列出所有 Agent，没装的不能选，给出安装命令。 */
export function AgentList({
  title,
  agents,
  selected,
  onPick,
}: {
  title: string;
  agents: AgentOption[];
  selected: string;
  onPick(id: string): void;
}) {
  const t = useT();
  return (
    <>
      <div className="menu-title">{title}</div>
      <div className="menu-list">
        {agents.map((a) =>
          a.installed ? (
            <button key={a.id} className="menu-item" onClick={() => onPick(a.id)}>
              <AgentIcon kind={a.id} size={14} />
              <span>{a.name}</span>
              {a.signedIn === false && <span className="meta">{t("setup.notSignedIn")}</span>}
              <span className="grow" />
              {a.signedIn === false && a.canSignIn && <SetupButton agent={a} />}
              {a.id === selected && <Check size={13} strokeWidth={2} />}
            </button>
          ) : (
            <div key={a.id} className="menu-item is-disabled" title={t("newTask.notInstalled")}>
              <AgentIcon kind={a.id} size={14} />
              <span>{a.name}</span>
              <span className="meta">{a.tooOld ? t("newTask.tooOld", a.tooOld) : t("newTask.notInstalled")}</span>
              <span className="grow" />
              {a.install && <CopyCommand command={a.install} />}
              {canSetUp(a) && <SetupButton agent={a} />}
            </div>
          ),
        )}
      </div>
    </>
  );
}

/** 一键安装（没装时）、升级（太旧时）或登录（没登录时）。 */
export function SetupButton({ agent }: { agent: AgentOption }) {
  const t = useT();
  const signIn = agent.installed;
  const title = t(signIn ? "setup.signInTitle" : agent.tooOld ? "setup.updateTitle" : "setup.installTitle", {
    agent: agent.name,
  });
  return (
    <span
      role="button"
      tabIndex={-1}
      className="icon-btn"
      title={title}
      aria-label={title}
      onClick={(e) => {
        e.stopPropagation();
        requestSetup(agent.id);
      }}
    >
      {signIn ? <LogIn size={13} strokeWidth={1.75} /> : <Download size={13} strokeWidth={1.75} />}
    </span>
  );
}

export function CopyCommand({ command }: { command: string }) {
  const t = useT();
  const [copied, setCopied] = useState(false);
  return (
    <button
      className="icon-btn"
      title={copied ? t("newTask.copied") : t("newTask.copyInstall", { command })}
      onClick={(e) => {
        e.stopPropagation();
        void navigator.clipboard.writeText(command).then(() => setCopied(true));
      }}
    >
      {copied ? <Check size={13} strokeWidth={2} /> : <Copy size={13} strokeWidth={1.75} />}
    </button>
  );
}

/** 挂在按钮下面的下拉框，点别处或按 Esc 关闭。 */
export function Popover({ children, onClose }: { children: React.ReactNode; onClose(): void }) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const close = (e: MouseEvent) => !ref.current?.parentElement?.contains(e.target as Node) && onClose();
    window.addEventListener("mousedown", close, true);
    return () => window.removeEventListener("mousedown", close, true);
  }, [onClose]);
  return (
    <div
      ref={ref}
      className="menu dropdown"
      onKeyDown={(e) => {
        if (e.key === "Escape") {
          e.stopPropagation();
          onClose();
        }
      }}
    >
      {children}
    </div>
  );
}
