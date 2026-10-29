import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useLocale, useT, type Translator } from "../../i18n";
import {
  REMOTE_CHANGED,
  TAILSCALE_DOWNLOAD_URL,
  appPortsOf,
  canPair,
  checkAppPort,
  formatCountdown,
  formatStamp,
  hangarPorts,
  newDevices,
  remoteAddress,
  remoteAppSlots,
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
  withAppPort,
  withoutAppPort,
  type AppPortProblem,
  type AppSlot,
  type PairStart,
  type RemoteDevice,
  type RemoteStatus,
  type TailscaleStatus,
} from "../../lib/remote";
import { useStore } from "../../store";
import { mobileOptions, setAutostart, setKeepAwake, type MobileOptions } from "./mobileOptions";
import "./MobileSettings.css";

/**
 * The "Mobile" page: turn Hangar Mobile on, get Tailscale ready, pair a phone,
 * then see and revoke the phones, and say which apps the phone may open. One
 * step at a time: each block below only appears once the one above it is done.
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
  busy: { toggle: boolean; serve: boolean; pair: boolean; options: boolean };
  error: string | null;
  /** Null until the PC has answered. */
  options: MobileOptions | null;
  onToggle: (enabled: boolean) => void;
  onKeepAwake: (enabled: boolean) => void;
  onAutostart: (enabled: boolean) => void;
  onServe: () => void;
  onRecheck: () => void;
  onOpenLink: (url: string) => void;
  onPairStart: () => void;
  onPairClose: () => void;
  onRevokeAsk: (id: string | null) => void;
  onRevoke: (id: string) => void;
  /** The workspaces whose apps the phone may open, the HQ left out, with the ports the person added. */
  appProjects: AppProject[];
  /** Null until the PC has answered. */
  slots: AppSlot[] | null;
  /** What is typed in each workspace's port field, by workspace id. */
  portDrafts: Record<string, string>;
  /** Why the port tried last could not be added, by workspace id. */
  portProblems: Record<string, AppPortProblem>;
  onPortDraft: (workspaceId: string, text: string) => void;
  onPortAdd: (workspaceId: string) => void;
  onPortRemove: (workspaceId: string, port: number) => void;
  onSlotsRefresh: () => void;
}

export interface AppProject {
  id: string;
  name: string;
  ports: number[];
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

const PROBLEM_KEY = {
  invalid: "mobile.appPortInvalid",
  range: "mobile.appPortRange",
  hangar: "mobile.appPortHangar",
  duplicate: "mobile.appPortDuplicate",
} as const;

/** A sentence with commands in it: each `{name}` becomes `<code>` around its value. */
function withCode(text: string, values: Record<string, string>): ReactNode[] {
  return text.split(/\{(\w+)\}/).map((part, index) =>
    index % 2 === 1 && part in values ? <code key={index}>{values[part]}</code> : part,
  );
}

/**
 * The ports the phone may open, per workspace: its workers' are found by
 * themselves, the person adds their own. Only the lists change here; the
 * window answers the phone from them (lib/remoteApps).
 */
function AppsBlock({ props }: { props: MobileViewProps }) {
  const { t, appProjects, portDrafts, portProblems } = props;
  return (
    <Block title={t("mobile.apps")} hint={t("mobile.appsHint")}>
      {appProjects.length === 0 && (
        <div className="set-row">
          <span className="set-row__hint">{t("mobile.appsNone")}</span>
        </div>
      )}
      {appProjects.map((project) => {
        const draft = portDrafts[project.id] ?? "";
        const problem = portProblems[project.id];
        return (
          <div className="set-row set-row--block mob-app" key={project.id}>
            <span className="set-row__label">{project.name}</span>
            <div className="mob-ports">
              {project.ports.length === 0 && <span className="set-row__hint">{t("mobile.appPortsNone")}</span>}
              {project.ports.map((port) => (
                <span className="mob-port" key={port}>
                  <span className="mob-port__number">{port}</span>
                  <button
                    type="button"
                    className="mob-port__remove"
                    aria-label={t("mobile.appPortRemove", { port: String(port), name: project.name })}
                    title={t("mobile.appPortRemove", { port: String(port), name: project.name })}
                    onClick={() => props.onPortRemove(project.id, port)}
                  >
                    ×
                  </button>
                </span>
              ))}
            </div>
            <form
              className="mob-port-add"
              onSubmit={(event) => {
                event.preventDefault();
                props.onPortAdd(project.id);
              }}
            >
              <input
                className="set-input mob-port-input"
                inputMode="numeric"
                placeholder="5173"
                aria-label={t("mobile.appPortLabel", { name: project.name })}
                aria-invalid={problem ? true : undefined}
                value={draft}
                onChange={(event) => props.onPortDraft(project.id, event.target.value)}
              />
              <button type="submit" className="btn" disabled={!draft.trim()}>
                {t("mobile.appPortAdd")}
              </button>
            </form>
            {problem && (
              <p className="mob-error mob-port-problem" role="alert">
                {t(PROBLEM_KEY[problem], { port: draft.trim() })}
              </p>
            )}
          </div>
        );
      })}
    </Block>
  );
}

/** The slots an app goes through, the apps they show, and what that changes in Tailscale. */
function SlotsBlock({ props }: { props: MobileViewProps }) {
  const { t, locale, now, slots } = props;
  if (!slots) return null;
  const shown = slots.filter((slot) => slot.app);
  const taken = slots.filter((slot) => slot.mapping === "taken");
  return (
    <Block title={t("mobile.appSlots")} hint={t("mobile.appSlotsHint")}>
      {shown.length === 0 && (
        <div className="set-row">
          <span className="set-row__hint">{t("mobile.appSlotsEmpty")}</span>
        </div>
      )}
      {shown.map((slot) => (
        <div className="set-row" key={slot.slot}>
          <span className="set-row__text">
            <span className="set-row__label">
              {t("mobile.appSlot", { workspace: slot.app!.workspace, port: String(slot.app!.port) })}
            </span>
            <span className="set-row__hint">
              {t("mobile.appSlotVia", {
                https: String(slot.httpsPort),
                date: formatStamp(slot.app!.lastUsed, locale, now),
              })}
            </span>
          </span>
        </div>
      ))}
      <div className="set-row set-row--block mob-step">
        {taken.map((slot) => (
          <p className="mob-note" key={slot.slot}>
            {t("mobile.appSlotTaken", { https: String(slot.httpsPort) })}
          </p>
        ))}
        <p className="mob-note">{t("mobile.appsTailscale")}</p>
        <p className="mob-note">
          {withCode(t("mobile.appsTailscaleRemove"), {
            command: "tailscale serve --https=8443 off",
            reset: "tailscale serve reset",
          })}
        </p>
        <div className="mob-actions">
          <button type="button" className="btn btn--ghost" onClick={props.onSlotsRefresh}>
            {t("mobile.recheck")}
          </button>
        </div>
      </div>
    </Block>
  );
}

function Switch({
  label,
  hint,
  checked,
  disabled,
  onChange,
}: {
  label: string;
  hint: string;
  checked: boolean;
  disabled: boolean;
  onChange: (checked: boolean) => void;
}) {
  return (
    <label className="set-row set-row--toggle">
      <span className="set-row__text">
        <span className="set-row__label">{label}</span>
        <span className="set-row__hint">{hint}</span>
      </span>
      <span className="set-row__control">
        <span className="switch">
          <input
            type="checkbox"
            role="switch"
            checked={checked}
            disabled={disabled}
            onChange={(e) => onChange(e.target.checked)}
          />
          <span className="switch__track" aria-hidden="true">
            <span className="switch__knob" />
          </span>
        </span>
      </span>
    </label>
  );
}

/** Leaving for a week: the PC kept awake, and Hangar there when it is back from a restart. */
function AwayBlock({ props }: { props: MobileViewProps }) {
  const { t, options, busy } = props;
  if (!options) return null;
  return (
    <Block title={t("mobile.away")} hint={t("mobile.awayHint")}>
      <Switch
        label={t("mobile.keepAwake")}
        hint={options.keepAwakeSupported ? t("mobile.keepAwakeHint") : t("mobile.keepAwakeUnsupported")}
        checked={options.keepAwake}
        disabled={busy.options || !options.keepAwakeSupported}
        onChange={props.onKeepAwake}
      />
      <Switch
        label={t("mobile.autostart")}
        hint={t("mobile.autostartHint")}
        checked={options.autostart}
        disabled={busy.options}
        onChange={props.onAutostart}
      />
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
          <AppsBlock props={props} />
          <SlotsBlock props={props} />
          <AwayBlock props={props} />
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
  const [options, setOptions] = useState<MobileOptions | null>(null);
  const [busy, setBusy] = useState({ toggle: false, serve: false, pair: false, options: false });
  const [error, setError] = useState<string | null>(null);
  const [now, setNow] = useState(() => Date.now());
  const { state, updateWorkspace } = useStore();
  const [slots, setSlots] = useState<AppSlot[] | null>(null);
  const [portDrafts, setPortDrafts] = useState<Record<string, string>>({});
  const [portProblems, setPortProblems] = useState<Record<string, AppPortProblem>>({});
  const appProjects = state.workspaces
    .filter((ws) => !ws.hq)
    .map((ws) => ({ id: ws.id, name: ws.name, ports: appPortsOf(ws) }));

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

  const refreshSlots = useCallback(() => remoteAppSlots().then(setSlots), []);

  const setPortProblem = (workspaceId: string, problem: AppPortProblem | null) =>
    setPortProblems(({ [workspaceId]: _was, ...rest }) => (problem ? { ...rest, [workspaceId]: problem } : rest));

  const addPort = (workspaceId: string) => {
    const project = appProjects.find((entry) => entry.id === workspaceId);
    if (!project) return;
    const checked = checkAppPort(portDrafts[workspaceId] ?? "", project.ports, hangarPorts(remote, slots));
    if ("problem" in checked) {
      setPortProblem(workspaceId, checked.problem);
      return;
    }
    updateWorkspace(workspaceId, { mobilePorts: withAppPort(project.ports, checked.port) });
    setPortDrafts(({ [workspaceId]: _typed, ...rest }) => rest);
    setPortProblem(workspaceId, null);
  };

  const removePort = (workspaceId: string, port: number) => {
    const project = appProjects.find((entry) => entry.id === workspaceId);
    if (!project) return;
    const left = withoutAppPort(project.ports, port);
    updateWorkspace(workspaceId, { mobilePorts: left.length ? left : undefined });
  };

  const guard = useCallback((work: () => Promise<unknown>) => {
    setError(null);
    return work().catch((err) => setError(message(err)));
  }, []);

  useEffect(() => {
    void guard(() => Promise.all([remoteStatus().then(setRemote), mobileOptions().then(setOptions)]));
  }, [guard]);

  // Tailscale and the list only matter while Hangar Mobile is on.
  useEffect(() => {
    if (!enabled) return;
    void guard(() => Promise.all([refreshTailscale(), refreshDevices(), refreshSlots()]));
  }, [enabled, guard, refreshTailscale, refreshDevices, refreshSlots]);

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
      options={options}
      onKeepAwake={(on) =>
        void work("options", async () => {
          await setKeepAwake(on);
          setOptions(await mobileOptions());
        })
      }
      onAutostart={(on) =>
        void work("options", async () => {
          await setAutostart(on);
          setOptions(await mobileOptions());
        })
      }
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
      appProjects={appProjects}
      slots={slots}
      portDrafts={portDrafts}
      portProblems={portProblems}
      onPortDraft={(workspaceId, text) => {
        setPortDrafts((drafts) => ({ ...drafts, [workspaceId]: text }));
        setPortProblem(workspaceId, null);
      }}
      onPortAdd={addPort}
      onPortRemove={removePort}
      onSlotsRefresh={() => void guard(refreshSlots)}
    />
  );
}
