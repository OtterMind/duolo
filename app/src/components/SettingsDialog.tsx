import { useEffect, useRef, useState } from "react";
import { Check as CheckIcon, Info, Mic, Palette, SlidersHorizontal, TriangleAlert, UserRound, X } from "lucide-react";

import { LOCALES, useI18n, type LanguagePref, type MessageKey } from "../i18n";
import { api, type Check, type FnKeyStatus, type Surface, type VoiceSettings } from "../lib/api";
import { agentOptions, defaultChoice, rememberChoice } from "../lib/agents";
import { isMac, TALK_KEY } from "../lib/platform";
import { allPlugins } from "../lib/plugins";
import {
  ACCENTS,
  appearance,
  FONT_SIZES,
  setAppearance,
  THEME_SWATCH,
  THEMES,
  type AccentId,
  type Appearance as AppearanceSettings,
  type ThemeId,
} from "../lib/theme";
import { AccountSettings } from "./AccountSettings";

const THEME_NAME: Record<ThemeId, MessageKey> = {
  dark: "settings.themeDark",
  light: "settings.themeLight",
  midnight: "settings.themeMidnight",
  mocha: "settings.themeMocha",
  system: "settings.themeSystem",
};
const ACCENT_NAME: Record<AccentId, MessageKey> = {
  blue: "settings.accentBlue",
  orange: "settings.accentOrange",
  green: "settings.accentGreen",
  purple: "settings.accentPurple",
  pink: "settings.accentPink",
};

type Tab = "general" | "appearance" | "voice" | "accounts" | "about";

const TAB_KEY = "duolo.settingsTab";

/** 设置（左下角齿轮、⌘,）：左边分类，右边内容。打开时回到上次看的那一类。 */
export function SettingsDialog({
  env,
  onClose,
  onError,
}: {
  /** 环境检查的结果：哪些 Agent 装了（默认 Agent 只能选装了的）。 */
  env: Check[] | null;
  onClose(): void;
  onError(err: unknown): void;
}) {
  const { t } = useI18n();
  const ref = useRef<HTMLDivElement>(null);
  const hasAccounts = allPlugins().some((p) => p.accounts);
  const tabs: { id: Tab; label: string; icon: typeof Mic }[] = [
    { id: "general", label: t("settings.general"), icon: SlidersHorizontal },
    { id: "appearance", label: t("settings.appearance"), icon: Palette },
    { id: "voice", label: t("settings.voice"), icon: Mic },
    ...(hasAccounts ? [{ id: "accounts" as const, label: t("account.title"), icon: UserRound }] : []),
    { id: "about", label: t("settings.about"), icon: Info },
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
            {tab === "general" && <General env={env} onError={onError} />}
            {tab === "appearance" && <Appearance />}
            {tab === "about" && <About onError={onError} />}
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

function General({ env, onError }: { env: Check[] | null; onError(err: unknown): void }) {
  const { pref, setPref, t } = useI18n();
  const [defaults, setDefaults] = useState<{ writer: string; reviewer: string; surface: Surface } | null>(null);
  const installed = agentOptions(env).filter((a) => a.installed);

  useEffect(() => {
    api.agentDefaults().then(
      (d) => setDefaults({ ...d, writer: defaultChoice(env, d).writer }),
      onError,
    );
  }, [env, onError]);

  /** 下拉框里要有当前值，哪怕它没装。 */
  const options = (selected: string, filter: (id: string) => boolean = () => true) => {
    const list = installed.filter((a) => filter(a.id));
    const missing = selected && !list.some((a) => a.id === selected);
    return (
      <>
        {missing && <option value={selected}>{t("settings.notInstalled", { agent: selected })}</option>}
        {list.map((a) => (
          <option key={a.id} value={a.id}>
            {a.name}
          </option>
        ))}
      </>
    );
  };

  return (
    <>
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
      <div className="settings-group-title">{t("settings.newTasks")}</div>
      <div className="settings-card">
        <label className="settings-row">
          <span className="grow">
            {t("settings.writer")}
            <span className="settings-hint">{t("settings.writerHint")}</span>
          </span>
          <select
            value={defaults?.writer ?? ""}
            disabled={!defaults}
            onChange={(e) => {
              rememberChoice({ writer: e.target.value });
              setDefaults((d) => d && { ...d, writer: e.target.value });
            }}
          >
            {defaults && options(defaults.writer)}
          </select>
        </label>
        <label className="settings-row">
          <span className="grow">{t("settings.surface")}</span>
          <select
            value={defaults?.surface ?? "terminal"}
            disabled={!defaults}
            onChange={(e) => {
              const surface = e.target.value as Surface;
              setDefaults((d) => d && { ...d, surface });
              api.setDefaultSurface(surface).catch(onError);
            }}
          >
            <option value="terminal">{t("settings.surfaceTerminal")}</option>
            <option value="chat">{t("settings.surfaceChat")}</option>
          </select>
        </label>
        <label className="settings-row">
          <span className="grow">{t("settings.reviewer")}</span>
          <select
            value={defaults?.reviewer ?? ""}
            disabled={!defaults}
            onChange={(e) => {
              const reviewer = e.target.value;
              setDefaults((d) => d && { ...d, reviewer });
              api.setDefaultReviewer(reviewer).catch(onError);
            }}
          >
            {defaults && options(defaults.reviewer, (id) => allPlugins().find((p) => p.id === id)?.reviews ?? true)}
          </select>
        </label>
      </div>
    </>
  );
}

function Appearance() {
  const { t } = useI18n();
  const [value, setValue] = useState<AppearanceSettings>(appearance);
  const change = (next: Partial<AppearanceSettings>) => {
    setAppearance(next);
    setValue(appearance());
  };
  return (
    <>
      <div className="settings-group-title">{t("settings.theme")}</div>
      <div className="theme-grid">
        {THEMES.map((id) => (
          <button
            key={id}
            className={value.theme === id ? "theme-card is-on" : "theme-card"}
            onClick={() => change({ theme: id })}
          >
            <ThemeSwatch id={id} />
            <span>{t(THEME_NAME[id])}</span>
          </button>
        ))}
      </div>
      <div className="settings-card">
        <div className="settings-row">
          <span className="grow">{t("settings.accent")}</span>
          <span className="accent-dots">
            {ACCENTS.map((a) => (
              <button
                key={a.id}
                className={value.accent === a.id ? "accent-dot is-on" : "accent-dot"}
                style={{ background: a.color }}
                title={t(ACCENT_NAME[a.id])}
                aria-label={t(ACCENT_NAME[a.id])}
                onClick={() => change({ accent: a.id })}
              >
                {value.accent === a.id && <CheckIcon size={11} strokeWidth={3} />}
              </button>
            ))}
          </span>
        </div>
        <label className="settings-row">
          <span className="grow">
            {t("settings.fontSize")}
            {value.theme === "light" && <span className="settings-hint">{t("settings.lightTerminal")}</span>}
          </span>
          <select value={value.fontSize} onChange={(e) => change({ fontSize: Number(e.target.value) })}>
            {FONT_SIZES.map((size) => (
              <option key={size} value={size}>
                {size}
              </option>
            ))}
          </select>
        </label>
      </div>
    </>
  );
}

/** 主题的小预览：侧边栏、底色和两行"文字"。跟随系统的左右各一半。 */
function ThemeSwatch({ id }: { id: (typeof THEMES)[number] }) {
  if (id === "system") {
    return (
      <span className="theme-swatch split">
        <ThemeSwatch id="dark" />
        <ThemeSwatch id="light" />
      </span>
    );
  }
  const [bg, side, text] = THEME_SWATCH[id];
  return (
    <span className="theme-swatch" style={{ background: bg }}>
      <span className="theme-swatch-side" style={{ background: side }} />
      <span className="theme-swatch-line" style={{ background: text }} />
      <span className="theme-swatch-line short" style={{ background: text }} />
    </span>
  );
}

function About({ onError }: { onError(err: unknown): void }) {
  const { t } = useI18n();
  const [info, setInfo] = useState<{ version: string; dataDir: string; configFile: string } | null>(null);
  useEffect(() => {
    api.appInfo().then(setInfo, onError);
  }, [onError]);
  const reveal = t(isMac ? "task.reveal" : "task.openFolder");
  return (
    <div className="settings-card">
      <div className="settings-row">
        <span className="grow">{t("settings.version")}</span>
        <span className="meta">{info?.version ?? ""}</span>
      </div>
      <div className="settings-row">
        <span className="grow">
          {t("settings.configFile")}
          <span className="settings-hint">{info?.configFile}</span>
        </span>
        <button className="btn small" disabled={!info} onClick={() => info && api.reveal(info.configFile).catch(onError)}>
          {reveal}
        </button>
      </div>
      <div className="settings-row">
        <span className="grow">
          {t("settings.dataDir")}
          <span className="settings-hint">{info?.dataDir}</span>
        </span>
        <button className="btn small" disabled={!info} onClick={() => info && api.reveal(info.dataDir).catch(onError)}>
          {reveal}
        </button>
      </div>
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
