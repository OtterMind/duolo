import { useCallback, useEffect, useRef, useState } from "react";
import { QrCode, Smartphone, Trash2 } from "lucide-react";

import { errorMessage, useT } from "../i18n";
import { api, type LinkStatus, type Pairing, type PhoneView } from "../lib/api";
import { Confirm, type ConfirmSpec } from "./Confirm";

/** 看一眼连接和手机的间隔：等手机扫码时要快一点。 */
const POLL_MS = 2000;

/** 设置里的"手机"：连接状态、配对二维码、配好对的手机。 */
export function PhoneSettings({ onError }: { onError(err: unknown): void }) {
  const t = useT();
  const [status, setStatus] = useState<LinkStatus | null>(null);
  const [phones, setPhones] = useState<PhoneView[]>([]);
  const [pairing, setPairing] = useState<Pairing | null>(null);
  const [making, setMaking] = useState(false);
  const [confirm, setConfirm] = useState<ConfirmSpec | null>(null);
  const known = useRef<Set<string> | null>(null);

  const refresh = useCallback(() => {
    api.linkStatus().then(setStatus, () => {});
    api.linkPhones().then((list) => {
      // 新配好一台：二维码用掉了，收起来。
      const ids = new Set(list.map((p) => p.id));
      if (known.current && list.some((p) => !known.current!.has(p.id))) setPairing(null);
      known.current = ids;
      setPhones(list);
    }, () => {});
  }, []);

  useEffect(() => {
    refresh();
    const timer = window.setInterval(refresh, POLL_MS);
    return () => window.clearInterval(timer);
  }, [refresh]);

  // 二维码过期了就收起来。
  useEffect(() => {
    if (!pairing) return;
    const timer = window.setTimeout(() => setPairing(null), Math.max(0, pairing.expiresAt - Date.now()));
    return () => window.clearTimeout(timer);
  }, [pairing]);

  const pair = async () => {
    setMaking(true);
    try {
      setPairing(await api.linkPair());
    } catch (err) {
      onError(err);
    } finally {
      setMaking(false);
    }
  };

  const remove = (phone: PhoneView) =>
    setConfirm({
      title: t("phone.removeTitle", { name: phone.name }),
      body: t("phone.removeBody"),
      action: t("phone.remove"),
      danger: true,
      run: async () => {
        await api.linkUnpair(phone.id);
        refresh();
      },
    });

  const off = status?.state === "off";
  return (
    <>
      <p className="account-intro">{t("phone.intro")}</p>
      <div className="settings-card">
        <div className="settings-row">
          <span className="grow">
            {t("phone.connection")}
            <span className="settings-hint">
              {status === null
                ? t("phone.connecting")
                : status.state === "online"
                  ? t("phone.online")
                  : status.state === "connecting"
                    ? t("phone.connecting")
                    : status.state === "off"
                      ? t("error.linkNotConfigured")
                      : errorMessage(t, { code: status.error })}
            </span>
          </span>
          <span className={`link-dot is-${status?.state ?? "connecting"}`} />
        </div>
      </div>

      <div className="settings-card">
        <div className="settings-row">
          <span className="grow">
            {t("phone.pair")}
            <span className="settings-hint">{t("phone.pairHint")}</span>
          </span>
          <button className="btn small primary" disabled={off || making} onClick={() => void pair()}>
            <QrCode size={14} strokeWidth={1.75} />
            {pairing ? t("phone.newCode") : t("phone.showCode")}
          </button>
        </div>
        {pairing && (
          <div className="pair-code">
            <img src={`data:image/svg+xml;charset=utf-8,${encodeURIComponent(pairing.svg)}`} alt={t("phone.pair")} width={220} height={220} />
            <p className="settings-hint">{t("phone.scanHint")}</p>
          </div>
        )}
      </div>

      <div className="settings-group-title">{t("phone.paired")}</div>
      <div className="settings-card">
        {phones.length === 0 ? (
          <div className="settings-row">
            <span className="grow settings-hint">{t("phone.none")}</span>
          </div>
        ) : (
          phones.map((phone) => (
            <div key={phone.id} className="settings-row">
              <Smartphone size={15} strokeWidth={1.75} className="phone-icon" />
              <span className="grow">
                {phone.name}
                <span className="settings-hint">{phone.online ? t("phone.onlineNow") : t("phone.offlineNow")}</span>
              </span>
              <button className="icon-btn" title={t("phone.remove")} aria-label={t("phone.remove")} onClick={() => remove(phone)}>
                <Trash2 size={14} strokeWidth={1.75} />
              </button>
            </div>
          ))
        )}
      </div>
      {confirm && <Confirm spec={confirm} onDone={() => setConfirm(null)} onError={onError} />}
    </>
  );
}
