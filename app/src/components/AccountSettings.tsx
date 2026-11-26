import { useCallback, useEffect, useState } from "react";
import { Check, LogIn, Plus, Trash2 } from "lucide-react";

import { useT } from "../i18n";
import { api, type AgentAccount } from "../lib/api";
import { allPlugins } from "../lib/plugins";
import { onSetupReady, requestSetup } from "../lib/setup";
import { AgentIcon } from "./AgentIcon";

/**
 * 设置里的"账号"：每种能用多个账号的 Agent 列出它的账号（默认的、在 Duolo 里添加的、别的软件里登录好的，例如 Orca），
 * 选一个用；额度用完时自动换下一个（见 `crates/duolo-engine/src/accounts.rs`）。
 */
export function AccountSettings({ onError }: { onError(err: unknown): void }) {
  const t = useT();
  const agents = allPlugins().filter((p) => p.accounts);
  const [auto, setAuto] = useState<boolean | null>(null);

  useEffect(() => {
    api.autoSwitchAccount().then(setAuto, onError);
  }, [onError]);

  if (agents.length === 0) return null;
  return (
    <>
      <div className="settings-head">{t("account.title")}</div>
      <label className="settings-row">
        <span className="grow">{t("account.autoSwitch")}</span>
        <input
          type="checkbox"
          className="switch"
          checked={auto ?? false}
          disabled={auto === null}
          onChange={(e) => {
            setAuto(e.target.checked);
            api.setAutoSwitchAccount(e.target.checked).catch(onError);
          }}
        />
      </label>
      {agents.map((p) => (
        <AgentAccounts key={p.id} agent={p.id} name={p.name} onError={onError} />
      ))}
    </>
  );
}

function AgentAccounts({ agent, name, onError }: { agent: string; name: string; onError(err: unknown): void }) {
  const t = useT();
  const [accounts, setAccounts] = useState<AgentAccount[] | null>(null);
  const load = useCallback(() => void api.agentAccounts(agent).then(setAccounts, onError), [agent, onError]);

  useEffect(load, [load]);
  // 添加、登录好一个账号后重新查。
  useEffect(() => onSetupReady((id) => id === agent && load()), [agent, load]);

  const label = (a: AgentAccount, i: number) =>
    a.email || (a.source === "default" ? t("account.default") : t("account.unnamed", { n: i }));
  const source = (a: AgentAccount) =>
    a.source === "default" ? t("account.default") : a.source === "duolo" ? "Duolo" : a.source;

  return (
    <div className="account-group">
      <div className="settings-row">
        <AgentIcon kind={agent} size={14} />
        <span className="grow">{name}</span>
        <button
          className="icon-btn"
          title={t("account.addTitle", { agent: name })}
          aria-label={t("account.addTitle", { agent: name })}
          onClick={() => requestSetup(agent, "new")}
        >
          <Plus size={14} strokeWidth={1.75} />
        </button>
      </div>
      {accounts === null && <div className="account-row meta">{t("common.pleaseWait")}</div>}
      {accounts?.map((a, i) => (
        <div key={a.id} className={a.chosen ? "account-row is-chosen" : "account-row"}>
          <button
            className="account-pick"
            title={a.id || undefined}
            onClick={() =>
              api
                .chooseAccount(agent, a.id)
                .then(() => setAccounts((list) => list && list.map((x) => ({ ...x, chosen: x.id === a.id }))))
                .catch(onError)
            }
          >
            <span className="account-check">{a.chosen && <Check size={12} strokeWidth={2} />}</span>
            <span className="account-name">{label(a, i)}</span>
            <span className="meta">{source(a)}</span>
          </button>
          {a.signedIn === false && (
            <button
              className="icon-btn"
              title={t("setup.signInTitle", { agent: name })}
              aria-label={t("setup.signInTitle", { agent: name })}
              onClick={() => requestSetup(agent, a.id)}
            >
              <LogIn size={13} strokeWidth={1.75} />
            </button>
          )}
          {a.source === "duolo" && (
            <button
              className="icon-btn"
              title={t("account.remove")}
              aria-label={t("account.remove")}
              onClick={() => api.removeAccount(agent, a.id).then(load, onError)}
            >
              <Trash2 size={13} strokeWidth={1.75} />
            </button>
          )}
        </div>
      ))}
    </div>
  );
}
