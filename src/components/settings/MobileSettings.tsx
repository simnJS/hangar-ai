import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useLocale, useT, type Translator } from "../../i18n";
import {
  REMOTE_CHANGED,
  TAILSCALE_DOWNLOAD_URL,
  canPair,
  formatCountdown,
  formatStamp,
  newDevices,
  remoteAddress,
  remoteDevices,
  remotePairStart,
  remoteRevoke,
  remoteSetEnabled,
  remoteStatus,
  remoteTailscale,
  remoteTailscaleServe,
  secondsLeft,
  sortDevices,
  tailscaleStep,
  type PairStart,
  type RemoteDevice,
  type RemoteStatus,
  type TailscaleStatus,
} from "../../lib/remote";
import "./MobileSettings.css";

/**
 * The "Mobile" page: turn Hangar Mobile on, get Tailscale ready, pair a phone,
 * then see and revoke the phones. One step at a time: each block below only
 * appears once the one above it is done.
 *
 * `MobileView` is the markup, from props alone, so every state can be drawn
 * without the PC's side; `MobileSettings` feeds it from the Tauri commands.
 */

export interface MobileViewProps {
  t: Translator;
  locale: string;
  now: number;
  /** Null until the PC has answered. */
  remote: RemoteStatus | null;
  tailscale: TailscaleStatus | null;
  devices: RemoteDevice[];
  pairing: PairStart | null;
  /** A phone that just paired, named for a moment. */
  justPaired: string | null;
  /** The device whose revocation is waiting for its second click. */
  revoking: string | null;
  busy: { toggle: boolean; serve: boolean; pair: boolean };
  error: string | null;
  onToggle: (enabled: boolean) => void;
  onServe: () => void;
  onRecheck: () => void;
  onOpenLink: (url: string) => void;
  onPairStart: () => void;
  onPairClose: () => void;
  onRevokeAsk: (id: string | null) => void;
  onRevoke: (id: string) => void;
}

function Block({
  title,
  hint,
  children,
}: {
  title: string;
  hint?: string;
  children: ReactNode;
}) {
  return (
    <section className="set-section">
      <header className="set-section__head">
        <h3 className="set-section__title">{title}</h3>
        {hint && <p className="set-section__hint">{hint}</p>}
      </header>
      <div className="set-group">{children}</div>
    </section>
  );
}

function TailscaleBlock({ props }: { props: MobileViewProps }) {
  const { t, tailscale, busy } = props;
  if (!tailscale) return null;
  const step = tailscaleStep(tailscale);
  const address = remoteAddress(tailscale);

  return (
    <Block title={t("mobile.tailscale")}>
      <div className="set-row set-row--block mob-step">
        {step === "missing" && (
          <>
            <p className="mob-text">{t("mobile.tailscaleMissing")}</p>
            <div className="mob-actions">
              <button
                type="button"
                className="btn btn--primary"
                onClick={() => props.onOpenLink(TAILSCALE_DOWNLOAD_URL)}
              >
                {t("mobile.tailscaleDownload")}
              </button>
              <button type="button" className="btn btn--ghost" onClick={props.onRecheck}>
                {t("mobile.recheck")}
              </button>
            </div>
          </>
        )}

        {step === "stopped" && (
          <>
            <p className="mob-text">{t("mobile.tailscaleStopped")}</p>
            <div className="mob-actions">
              <button type="button" className="btn" onClick={props.onRecheck}>
                {t("mobile.recheck")}
              </button>
            </div>
          </>
        )}

        {step === "unserved" && (
          <>
            <p className="mob-text">{t("mobile.tailscaleUnserved")}</p>
            <div className="mob-actions">
              {/* Only ever this click: nothing runs `tailscale serve` on its own. */}
              <button
                type="button"
                className="btn btn--primary"
                disabled={busy.serve}
                onClick={props.onServe}
              >
                {busy.serve ? t("mobile.serving") : t("mobile.serve")}
              </button>
            </div>
            <p className="mob-note">{t("mobile.serveHint")}</p>
          </>
        )}

        {step === "ready" && address && (
          <>
            <p className="mob-text mob-ok">
              <i className="mob-dot" aria-hidden="true" />
              {t("mobile.tailscaleReady")}
            </p>
            <code className="mob-address">{address}</code>
          </>
        )}
      </div>
    </Block>
  );
}

function PairBlock({ props }: { props: MobileViewProps }) {
  const { t, pairing, now, busy, remote, tailscale } = props;
  const ready = canPair(remote, tailscale);
  const left = pairing ? secondsLeft(pairing.expiresAt, now) : 0;
  const expired = !!pairing && left === 0;

  return (
    <Block title={t("mobile.pair")} hint={t("mobile.pairHint")}>
      <div className="set-row set-row--block mob-step">
        {props.justPaired && (
          <p className="mob-text mob-ok" role="status">
            <i className="mob-dot" aria-hidden="true" />
            {t("mobile.paired", { name: props.justPaired })}
          </p>
        )}

        {!pairing && (
          <>
            <div className="mob-actions">
              <button
                type="button"
                className="btn btn--primary"
                disabled={!ready || busy.pair}
                onClick={props.onPairStart}
              >
                {busy.pair ? t("mobile.pairBusy") : t("mobile.pairShow")}
              </button>
            </div>
            {!ready && <p className="mob-note">{t("mobile.pairBlocked")}</p>}
          </>
        )}

        {pairing && (
          <div className="mob-pair">
            <div
              className={`mob-qr ${expired ? "is-expired" : ""}`}
              role="img"
              aria-label={t("mobile.pairQr")}
              // Drawn by our own backend (pairing.rs), not by anything a phone sends.
              dangerouslySetInnerHTML={{ __html: pairing.qrSvg }}
            />
            <div className="mob-pair__side">
              <span className="mob-note">{t("mobile.pairCode")}</span>
              <strong className="mob-code">{pairing.code}</strong>
              <span className={`mob-count ${expired ? "is-expired" : ""}`}>
                {expired
                  ? t("mobile.pairExpired")
                  : t("mobile.pairExpires", { time: formatCountdown(left) })}
              </span>
              <div className="mob-actions">
                {expired && (
                  <button
                    type="button"
                    className="btn btn--primary"
                    disabled={busy.pair}
                    onClick={props.onPairStart}
                  >
                    {t("mobile.pairAgain")}
                  </button>
                )}
                <button type="button" className="btn btn--ghost" onClick={props.onPairClose}>
                  {t("mobile.pairClose")}
                </button>
              </div>
            </div>
          </div>
        )}
      </div>
    </Block>
  );
}

function DevicesBlock({ props }: { props: MobileViewProps }) {
  const { t, locale, now, devices, revoking } = props;

  return (
    <Block title={t("mobile.devices")}>
      {devices.length === 0 && (
        <div className="set-row">
          <span className="set-row__hint">{t("mobile.devicesEmpty")}</span>
        </div>
      )}
      {devices.map((device) => (
        <div className="set-row" key={device.id}>
          <span className="set-row__text">
            <span className="set-row__label">{device.name}</span>
            <span className="set-row__hint">
              {revoking === device.id
                ? t("mobile.revokeHint")
                : [
                    t("mobile.devicePaired", { date: formatStamp(device.created, locale, now) }),
                    device.lastSeen > 0
                      ? t("mobile.deviceSeen", { date: formatStamp(device.lastSeen, locale, now) })
                      : t("mobile.deviceNeverSeen"),
                  ].join(" · ")}
            </span>
          </span>
          <span className="set-row__control">
            {revoking === device.id ? (
              <>
                <span className="mob-ask">{t("mobile.revokeAsk", { name: device.name })}</span>
                <button
                  type="button"
                  className="btn btn--ghost"
                  onClick={() => props.onRevokeAsk(null)}
                >
                  {t("create.cancel")}
                </button>
                <button
                  type="button"
                  className="btn btn--danger"
                  onClick={() => props.onRevoke(device.id)}
                >
                  {t("mobile.revoke")}
                </button>
              </>
            ) : (
              <button
                type="button"
                className="btn btn--delete"
                onClick={() => props.onRevokeAsk(device.id)}
              >
                {t("mobile.revoke")}
              </button>
            )}
          </span>
        </div>
      ))}
    </Block>
  );
}

export function MobileView(props: MobileViewProps) {
  const { t, remote, busy } = props;
  const enabled = !!remote?.enabled;
  const failure = props.error ?? remote?.error ?? null;

  return (
    <div className="mob">
      <section className="set-section">
        <div className="set-group">
          <label className="set-row set-row--toggle">
            <span className="set-row__text">
              <span className="set-row__label">{t("mobile.switch")}</span>
              <span className="set-row__hint">{t("mobile.switchHint")}</span>
            </span>
            <span className="set-row__control">
              <span className="switch">
                <input
                  type="checkbox"
                  role="switch"
                  checked={enabled}
                  disabled={!remote || busy.toggle}
                  onChange={(e) => props.onToggle(e.target.checked)}
                />
                <span className="switch__track" aria-hidden="true">
                  <span className="switch__knob" />
                </span>
              </span>
            </span>
          </label>
        </div>
        {failure && (
          <p className="mob-error" role="alert">
            {t("mobile.error", { error: failure })}
          </p>
        )}
      </section>

      {enabled && (
        <>
          <TailscaleBlock props={props} />
          <PairBlock props={props} />
          <DevicesBlock props={props} />
          <Block title={t("mobile.iphone")}>
            <div className="set-row set-row--block mob-step">
              <p className="mob-text">{t("mobile.iphoneOpen")}</p>
              <p className="mob-note">{t("mobile.iphoneNotify")}</p>
            </div>
          </Block>
        </>
      )}
    </div>
  );
}

const message = (err: unknown) => (err instanceof Error ? err.message : String(err));

/** The page itself: reads the PC's side, and keeps it fresh while it is open. */
export function MobileSettings() {
  const t = useT();
  const locale = useLocale();
  const [remote, setRemote] = useState<RemoteStatus | null>(null);
  const [tailscale, setTailscale] = useState<TailscaleStatus | null>(null);
  const [devices, setDevices] = useState<RemoteDevice[]>([]);
  const [pairing, setPairing] = useState<PairStart | null>(null);
  const [justPaired, setJustPaired] = useState<string | null>(null);
  const [revoking, setRevoking] = useState<string | null>(null);
  const [busy, setBusy] = useState({ toggle: false, serve: false, pair: false });
  const [error, setError] = useState<string | null>(null);
  const [now, setNow] = useState(() => Date.now());

  // The refresh a pairing compares against: who was there when the code was drawn.
  const known = useRef<Set<string>>(new Set());
  const pairingOpen = useRef(false);
  pairingOpen.current = pairing !== null;

  const enabled = !!remote?.enabled;

  const refreshDevices = useCallback(async () => {
    const list = sortDevices(await remoteDevices());
    setDevices(list);
    if (pairingOpen.current) {
      const joined = newDevices(known.current, list);
      if (joined.length > 0) {
        setPairing(null);
        setJustPaired(joined[0].name);
      }
    }
    known.current = new Set(list.map((device) => device.id));
  }, []);

  const refreshTailscale = useCallback(
    () => remoteTailscale().then(setTailscale),
    [],
  );

  const guard = useCallback((work: () => Promise<unknown>) => {
    setError(null);
    return work().catch((err) => setError(message(err)));
  }, []);

  useEffect(() => {
    void guard(() => remoteStatus().then(setRemote));
  }, [guard]);

  // Tailscale and the list only matter while Hangar Mobile is on.
  useEffect(() => {
    if (!enabled) return;
    void guard(() => Promise.all([refreshTailscale(), refreshDevices()]));
  }, [enabled, guard, refreshTailscale, refreshDevices]);

  // A phone paired or revoked: the PC says so, the list redraws.
  useEffect(() => {
    if (!enabled) return;
    const stop = listen(REMOTE_CHANGED, () => void guard(refreshDevices));
    return () => void stop.then((off) => off());
  }, [enabled, guard, refreshDevices]);

  // The countdown is the only thing here that moves on its own.
  useEffect(() => {
    if (!pairing) return;
    setNow(Date.now());
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, [pairing]);

  // A flash, not a state to dismiss.
  useEffect(() => {
    if (!justPaired) return;
    const timer = window.setTimeout(() => setJustPaired(null), 6000);
    return () => window.clearTimeout(timer);
  }, [justPaired]);

  const work = (key: keyof typeof busy, job: () => Promise<unknown>) => {
    setBusy((b) => ({ ...b, [key]: true }));
    return guard(job).finally(() => setBusy((b) => ({ ...b, [key]: false })));
  };

  return (
    <MobileView
      t={t}
      locale={locale}
      now={now}
      remote={remote}
      tailscale={tailscale}
      devices={devices}
      pairing={pairing}
      justPaired={justPaired}
      revoking={revoking}
      busy={busy}
      error={error}
      onToggle={(on) =>
        void work("toggle", async () => {
          setRemote(await remoteSetEnabled(on));
          if (!on) {
            setPairing(null);
            setRevoking(null);
          }
        })
      }
      onServe={() => void work("serve", () => remoteTailscaleServe().then(setTailscale))}
      onRecheck={() => void guard(refreshTailscale)}
      onOpenLink={(url) => void guard(() => openUrl(url))}
      onPairStart={() =>
        void work("pair", async () => {
          setJustPaired(null);
          known.current = new Set(devices.map((device) => device.id));
          setPairing(await remotePairStart());
        })
      }
      onPairClose={() => setPairing(null)}
      onRevokeAsk={setRevoking}
      onRevoke={(id) =>
        void guard(async () => {
          await remoteRevoke(id);
          setRevoking(null);
          await refreshDevices();
        })
      }
    />
  );
}
