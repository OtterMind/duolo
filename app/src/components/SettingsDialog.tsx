import { useEffect, useRef, useState } from "react";
import { Mic, SlidersHorizontal, TriangleAlert, UserRound, X } from "lucide-react";

import { LOCALES, useI18n, type LanguagePref } from "../i18n";
import { api, type FnKeyStatus, type VoiceSettings } from "../lib/api";
import { TALK_KEY } from "../lib/platform";
import { allPlugins } from "../lib/plugins";
import { AccountSettings } from "./AccountSettings";

type Tab = "general" | "voice" | "accounts";

const TAB_KEY = "duolo.settingsTab";

/** 设置（左下角齿轮、⌘,）：左边分类，右边内容。打开时回到上次看的那一类。 */
export function SettingsDialog({ onClose, onError }: { onClose(): void; onError(err: unknown): void }) {
  const { t } = useI18n();
  const ref = useRef<HTMLDivElement>(null);
  const hasAccounts = allPlugins().some((p) => p.accounts);
  const tabs: { id: Tab; label: string; icon: typeof Mic }[] = [
    { id: "general", label: t("settings.general"), icon: SlidersHorizontal },
    { id: "voice", label: t("settings.voice"), icon: Mic },
    ...(hasAccounts ? [{ id: "accounts" as const, label: t("account.title"), icon: UserRound }] : []),
  ];
  const [tab, setTab] = useState<Tab>(() => {
    const saved = readTab();
    return tabs.some((x) => x.id === saved) ? saved! : "general";
  });

  useEffect(() => ref.current?.focus(), []);
  useEffect(() => {
    const key = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        onClose();
      }
    };
    window.addEventListener("keydown", key, true);
    return () => window.removeEventListener("keydown", key, true);
  }, [onClose]);

  const pick = (id: Tab) => {
    setTab(id);
    try {
      localStorage.setItem(TAB_KEY, id);
    } catch {
      // 记不住就下次从"通用"开始。
    }
  };

  return (
    <div className="scrim" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div ref={ref} tabIndex={-1} className="dialog settings-dialog" role="dialog" aria-label={t("settings.title")}>
        <nav className="settings-nav">
          <div className="settings-title">{t("settings.title")}</div>
          {tabs.map(({ id, label, icon: Icon }) => (
            <button key={id} className={tab === id ? "settings-tab is-active" : "settings-tab"} onClick={() => pick(id)}>
              <Icon size={14} strokeWidth={1.75} />
              {label}
            </button>
          ))}
        </nav>
        <section className="settings-body">
          <header className="settings-body-head">
            <h2>{tabs.find((x) => x.id === tab)?.label}</h2>
            <button className="icon-btn" title={t("common.collapse")} aria-label={t("common.collapse")} onClick={onClose}>
              <X size={15} strokeWidth={1.75} />
            </button>
          </header>
          <div className="settings-pane">
            {tab === "general" && <General />}
            {tab === "voice" && <Voice onError={onError} />}
            {tab === "accounts" && <AccountSettings onError={onError} />}
          </div>
        </section>
      </div>
    </div>
  );
}

function readTab(): Tab | null {
  try {
    return localStorage.getItem(TAB_KEY) as Tab | null;
  } catch {
    return null;
  }
}

function General() {
  const { pref, setPref, t } = useI18n();
  return (
    <div className="settings-card">
      <label className="settings-row">
        <span className="grow">{t("settings.language")}</span>
        <select value={pref} onChange={(e) => setPref(e.target.value as LanguagePref)}>
          <option value="auto">{t("sidebar.languageAuto")}</option>
          {LOCALES.map((l) => (
            <option key={l.code} value={l.code}>
              {l.name}
            </option>
          ))}
        </select>
      </label>
    </div>
  );
}

function Voice({ onError }: { onError(err: unknown): void }) {
  const { t } = useI18n();
  const [voice, setVoice] = useState<VoiceSettings | null>(null);
  const fn = useFnKeyStatus();

  useEffect(() => {
    api.getVoiceSettings().then(setVoice, onError);
  }, [onError]);

  const saveVoice = (next: VoiceSettings) => {
    setVoice(next);
    api.setVoiceSettings(next).catch(onError);
  };

  return (
    <>
      {fn && !fn.trusted && (
        <div className="settings-note">
          <TriangleAlert size={13} strokeWidth={1.75} />
          <span className="grow">{t("settings.fnNeedsPermission")}</span>
          <button
            className="link-btn"
            onClick={() => {
              void api.requestFnKeyPermission();
              void api.openSystemSettings("accessibility");
            }}
          >
            {t("settings.allow")}
          </button>
        </div>
      )}
      {fn && fn.conflict && (
        <div className="settings-note">
          <TriangleAlert size={13} strokeWidth={1.75} />
          <span className="grow">{t("settings.fnConflict")}</span>
          <button className="link-btn" onClick={() => void api.openSystemSettings("keyboard")}>
            {t("settings.change")}
          </button>
        </div>
      )}
      <div className="settings-card">
        <div className="settings-row">
          <span className="grow">{t("settings.hotkey")}</span>
          <span className="meta">
            {t("settings.holdKey")} <kbd>{TALK_KEY}</kbd>
          </span>
        </div>
        <label className="settings-row">
          <span className="grow">{t("settings.organize")}</span>
          <input
            type="checkbox"
            className="switch"
            checked={voice?.organize ?? false}
            disabled={!voice}
            onChange={(e) => voice && saveVoice({ ...voice, organize: e.target.checked })}
          />
        </label>
      </div>
    </>
  );
}

/** 按住 Fn 说话的状态。用户可能正在系统设置里授权，打开期间每 2 秒刷新一次。 */
export function useFnKeyStatus() {
  const [status, setStatus] = useState<FnKeyStatus | null>(null);
  useEffect(() => {
    let alive = true;
    const load = () => api.fnKeyStatus().then((s) => alive && setStatus(s), () => {});
    void load();
    const id = window.setInterval(load, 2000);
    return () => {
      alive = false;
      window.clearInterval(id);
    };
  }, []);
  return status;
}
