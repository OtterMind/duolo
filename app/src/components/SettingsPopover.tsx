import { useEffect, useRef, useState } from "react";
import { TriangleAlert } from "lucide-react";

import { LOCALES, useI18n, type LanguagePref } from "../i18n";
import { api, type FnKeyStatus, type VoiceSettings } from "../lib/api";
import { TALK_KEY } from "../lib/platform";

/** 左下角齿轮打开的设置：界面语言，以及语音输入的快捷键、整理。 */
export function SettingsPopover({ onClose, onError }: { onClose(): void; onError(err: unknown): void }) {
  const { pref, setPref, t } = useI18n();
  const ref = useRef<HTMLDivElement>(null);
  const [voice, setVoice] = useState<VoiceSettings | null>(null);
  const fn = useFnKeyStatus();

  useEffect(() => {
    api.getVoiceSettings().then(setVoice, onError);
  }, [onError]);

  useEffect(() => {
    const close = (e: MouseEvent) => !ref.current?.contains(e.target as Node) && onClose();
    const key = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("mousedown", close, true);
    window.addEventListener("keydown", key, true);
    return () => {
      window.removeEventListener("mousedown", close, true);
      window.removeEventListener("keydown", key, true);
    };
  }, [onClose]);

  const saveVoice = (next: VoiceSettings) => {
    setVoice(next);
    api.setVoiceSettings(next).catch(onError);
  };

  return (
    <div ref={ref} className="menu settings" role="dialog" aria-label={t("settings.title")}>
      <div className="settings-head">{t("settings.general")}</div>
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

      <div className="settings-head">{t("settings.voice")}</div>
      <div className="settings-row">
        <span className="grow">{t("settings.hotkey")}</span>
        <span>
          {t("settings.holdKey")} <kbd>{TALK_KEY}</kbd>
        </span>
      </div>
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
