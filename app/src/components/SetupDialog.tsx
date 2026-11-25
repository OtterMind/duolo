import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { CircleCheck, TriangleAlert } from "lucide-react";

import { useT } from "../i18n";
import { api, type SetupStep } from "../lib/api";
import { agentOptions, type AgentOption } from "../lib/agents";
import { notifySetupReady } from "../lib/setup";
import { FONT_FAMILY, THEME } from "../lib/terminals";
import { AgentIcon } from "./AgentIcon";

type Phase = { kind: "running" } | { kind: "done" } | { kind: "failed"; why: string };

/**
 * 一键安装 Agent、登录账号：在窗口里的终端运行插件声明的命令（参考 T3 Code）。
 * 没装的先装，装好后查一遍，没登录的接着登录；登录会打开浏览器，要粘贴授权码时直接在终端里粘贴。
 * 失败时终端里的输出留着，看得到原因。
 */
export function SetupDialog({ agent, onClose }: { agent: AgentOption; onClose(): void }) {
  const t = useT();
  /** 第几次运行哪一步：换一步、重试都是新的一次，开一个新终端。 */
  const [run, setRun] = useState<{ step: SetupStep; n: number }>({
    step: agent.installed ? "login" : "install",
    n: 0,
  });
  const [phase, setPhase] = useState<Phase>({ kind: "running" });
  const host = useRef<HTMLDivElement>(null);
  const done = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    if (!host.current) return;
    const { step } = run;
    const term = new Terminal({
      fontFamily: FONT_FAMILY,
      fontSize: 12.5,
      lineHeight: 1.2,
      cursorStyle: "bar",
      allowTransparency: true,
      scrollback: 2000,
      theme: THEME,
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.open(host.current);
    fit.fit();
    term.focus();
    const input = term.onData((data) => void api.setupWrite(data).catch(() => {}));
    const resize = new ResizeObserver(() => {
      fit.fit();
      void api.setupResize(term.rows, term.cols).catch(() => {});
    });
    resize.observe(host.current);
    let current = true;
    const exited = listen<{ success: boolean }>("setup-exit", (e) => {
      if (current) void next(step, e.payload.success);
    });
    api
      .setupStart(agent.id, step, term.rows, term.cols, (bytes) => term.write(bytes))
      .catch((err: unknown) => current && setPhase({ kind: "failed", why: String(err) }));
    return () => {
      current = false;
      input.dispose();
      resize.disconnect();
      void exited.then((f) => f());
      void api.setupStop().catch(() => {});
      term.dispose();
    };
  }, [run]);

  /** 一步结束：重新检查环境，决定下一步。 */
  const next = async (finished: SetupStep, success: boolean) => {
    const fresh = agentOptions(await api.environment().catch(() => [])).find((a) => a.id === agent.id);
    if (!success) return setPhase({ kind: "failed", why: t("setup.failed") });
    if (!fresh?.installed) return setPhase({ kind: "failed", why: t("setup.notFound", { agent: agent.name }) });
    if (fresh.signedIn === false) {
      // 刚装好，接着登录；登录完了还是没登录上，就是没完成。
      if (finished === "install") return setRun((r) => ({ step: "login", n: r.n + 1 }));
      return setPhase({ kind: "failed", why: t("setup.failed") });
    }
    setPhase({ kind: "done" });
    notifySetupReady(agent.id);
  };

  useEffect(() => {
    if (phase.kind === "done") done.current?.focus();
  }, [phase.kind]);

  const title =
    phase.kind === "done"
      ? t("setup.ready", { agent: agent.name })
      : t(run.step === "install" ? "setup.installTitle" : "setup.signInTitle", { agent: agent.name });
  const hint =
    run.step === "install"
      ? t("setup.installHint", { command: agent.install })
      : t("setup.signInHint", { agent: agent.name });

  return (
    <div className="scrim" onMouseDown={(e) => e.target === e.currentTarget && phase.kind !== "running" && onClose()}>
      <div className="dialog setup-dialog" role="dialog" aria-label={title}>
        <h2 className="setup-title">
          <AgentIcon kind={agent.id} size={16} />
          {title}
        </h2>
        <p>{hint}</p>
        <div ref={host} className="setup-terminal" />
        {phase.kind === "done" && (
          <p className="setup-result">
            <CircleCheck size={15} strokeWidth={1.75} className="ok" />
            {t("setup.readyBody", { agent: agent.name })}
          </p>
        )}
        {phase.kind === "failed" && (
          <p className="setup-result">
            <TriangleAlert size={15} strokeWidth={1.75} className="warn" />
            {phase.why}
          </p>
        )}
        <div className="dialog-actions">
          {phase.kind === "running" && (
            <button className="btn" onClick={onClose}>
              {t("common.cancel")}
            </button>
          )}
          {phase.kind === "failed" && (
            <>
              <button className="btn" onClick={onClose}>
                {t("common.collapse")}
              </button>
              <button
                className="btn primary"
                onClick={() => {
                  setPhase({ kind: "running" });
                  setRun((r) => ({ ...r, n: r.n + 1 }));
                }}
              >
                {t("setup.retry")}
              </button>
            </>
          )}
          {phase.kind === "done" && (
            <button ref={done} className="btn primary" onClick={onClose}>
              {t("setup.done")}
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
