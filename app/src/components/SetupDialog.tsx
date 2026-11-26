import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { CircleCheck, TriangleAlert } from "lucide-react";

import { useT } from "../i18n";
import { api, type Check, type SetupStep } from "../lib/api";
import { agentOptions, firstSetupStep, type AgentOption } from "../lib/agents";
import { notifySetupReady } from "../lib/setup";
import { FONT_FAMILY, THEME } from "../lib/terminals";
import { AgentIcon } from "./AgentIcon";

type Phase = { kind: "running" } | { kind: "done" } | { kind: "failed"; why: string };

/** 登录时隔多久查一次登录了没有。 */
const SIGN_IN_POLL_MS = 3000;

const TITLE = {
  install: "setup.installTitle",
  update: "setup.updateTitle",
  login: "setup.signInTitle",
} as const satisfies Record<SetupStep, string>;

/**
 * 一键安装、升级 Agent，登录账号：在窗口里的终端运行插件声明的命令（参考 T3 Code、Orca）。
 * 没装的先装（太旧的用它自己的升级命令），装好后查一遍，没登录的接着登录；登录会打开浏览器，
 * 要粘贴授权码时直接在终端里粘贴。失败时终端里的输出留着，看得到原因。
 */
export function SetupDialog({
  agent,
  account,
  onEnv,
  onClose: close,
}: {
  agent: AgentOption;
  /** 只登录这个账号（`new` 是新添加一个，见 `api.setupStart`）。 */
  account?: string;
  /** 每步结束后重新检查的环境，交给 App 更新。 */
  onEnv(env: Check[]): void;
  onClose(): void;
}) {
  const t = useT();
  /** 第几次运行哪一步：换一步、重试都是新的一次，开一个新终端。 */
  const [run, setRun] = useState<{ step: SetupStep; n: number }>({
    step: account === undefined ? firstSetupStep(agent) : "login",
    n: 0,
  });
  /** 登录的账号：新添加的开始后换成它的文件夹，重试时还登录这一个。 */
  const target = useRef(account);
  /** 这次新添加的账号：没登录好就关掉时删掉，不留下空的账号。 */
  const added = useRef<string | null>(null);
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
    let finished = false;
    const finish = (success: boolean) => {
      if (!current || finished) return;
      finished = true;
      void next(step, success);
    };
    // 先订阅再开始：命令出错时可能马上就结束了。只认这次开始的那个（事件带着第几次）。
    let runId: number | null = null;
    const exited = listen<{ run: number; success: boolean }>("setup-exit", (e) => {
      if (e.payload.run === runId) finish(e.payload.success);
    });
    void exited
      .then(() => api.setupStart(agent.id, step, target.current, term.rows, term.cols, (bytes) => term.write(bytes)))
      .then((started) => {
        runId = started.run;
        if (started.account) target.current = added.current = started.account;
      })
      .catch((err: unknown) => current && setPhase({ kind: "failed", why: String(err) }));
    // 登录：浏览器授权完，有的 CLI 不会自己退出（Windows 上的 Codex，Orca 也是这么处理的）。查到登录了就算完成。
    const poll =
      step === "login" && agent.canSignIn
        ? window.setInterval(() => {
            void api.setupSignedIn(agent.id, target.current).then((ok) => {
              if (ok !== true || !current || finished) return;
              void api.setupStop().catch(() => {});
              finish(true);
            }, () => {});
          }, SIGN_IN_POLL_MS)
        : undefined;
    return () => {
      current = false;
      window.clearInterval(poll);
      input.dispose();
      resize.disconnect();
      void exited.then((f) => f());
      void api.setupStop().catch(() => {});
      term.dispose();
    };
  }, [run]);

  /** 一步结束：重新检查环境，决定下一步。 */
  const next = async (finished: SetupStep, success: boolean) => {
    const env = await api.environment().catch(() => null);
    if (env) onEnv(env);
    const fresh = env && agentOptions(env).find((a) => a.id === agent.id);
    if (!success) return setPhase({ kind: "failed", why: t("setup.failed") });
    // 登录某个账号：只看这个账号登录好了没有。
    if (account !== undefined) {
      const ok = await api.setupSignedIn(agent.id, target.current).catch(() => null);
      if (ok === false) return setPhase({ kind: "failed", why: t("setup.failed") });
      added.current = null;
      setPhase({ kind: "done" });
      notifySetupReady(agent.id);
      return;
    }
    if (!fresh?.installed) {
      const why = fresh?.tooOld ? t("newTask.tooOld", fresh.tooOld) : t("setup.notFound", { agent: agent.name });
      return setPhase({ kind: "failed", why });
    }
    // 刚装好的接着登录（查不出登录状态的也登录一次：刚装的不可能登录过）；升级完只有确实没登录的才登录。
    const needsSignIn =
      fresh.canSignIn &&
      (finished === "install" ? fresh.signedIn !== true : finished === "update" && fresh.signedIn === false);
    if (needsSignIn) return setRun((r) => ({ step: "login", n: r.n + 1 }));
    if (finished === "login" && fresh.signedIn === false) return setPhase({ kind: "failed", why: t("setup.failed") });
    setPhase({ kind: "done" });
    notifySetupReady(agent.id);
  };

  useEffect(() => {
    if (phase.kind === "done") done.current?.focus();
  }, [phase.kind]);

  const onClose = () => {
    if (added.current) void api.removeAccount(agent.id, added.current).catch(() => {});
    close();
  };

  const adding = account === "new";
  const title =
    phase.kind === "done"
      ? t(adding ? "account.added" : "setup.ready", { agent: agent.name })
      : t(adding ? "account.addTitle" : TITLE[run.step], { agent: agent.name });
  const hint = adding
    ? t("account.addHint", { agent: agent.name })
    : {
        install: t("setup.installHint", { command: agent.install }),
        update: t("setup.updateHint", { agent: agent.name }),
        login: t("setup.signInHint", { agent: agent.name }),
      }[run.step];

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
            {t(account === undefined ? "setup.readyBody" : "account.addedBody", { agent: agent.name })}
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
