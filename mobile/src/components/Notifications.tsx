import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { ApiError, type Client } from "../api";
import { errorText } from "../errors";
import { locale, t, type Key } from "../i18n";
import {
  environment,
  pushStatus,
  pushSupport,
  refusedByPerson,
  sameKey,
  subscribe,
  workerRegistration,
  type PushStatus,
} from "../push";
import { useRemoteContext } from "../remote";
import { load, save } from "../storage";
import { Icon } from "./Icon";

const permissionNow = (): NotificationPermission | null =>
  typeof Notification === "undefined" ? null : Notification.permission;

/** The PC's refusal says why; the browser's own trouble has no words worth showing. */
const why = (cause: unknown) => (cause instanceof ApiError ? errorText(cause) : t("push.browserFailed"));

interface Prepared {
  registration: ServiceWorkerRegistration | null;
  key: string | null;
}

/** The subscription is handed to the PC once a launch, not at every visit to For you. */
let handedOver = false;

/**
 * This phone's notifications: whether it has them, and turning them on and
 * off. When the app opens, a subscription already there is handed to the PC
 * again (it may have lost it, or the language changed), or made anew if the
 * PC's key changed; the worker and the key are read then, ready for a tap.
 */
export function usePush(client: Client) {
  const support = useMemo(() => pushSupport(environment()), []);
  const [permission, setPermission] = useState(permissionNow);
  const [subscribed, setSubscribed] = useState<boolean | null>(null);
  const [noWorker, setNoWorker] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const prepared = useRef<Prepared>({ registration: null, key: null });

  useEffect(() => {
    if (support !== "ok") return;
    let cancelled = false;
    void (async () => {
      const [registration, key] = await Promise.all([
        workerRegistration(),
        client
          .pushKey()
          .then((answer) => answer.key)
          .catch(() => null),
      ]);
      prepared.current = { registration, key };
      if (cancelled) return;
      if (!registration) {
        setNoWorker(true);
        return;
      }
      let subscription = await registration.pushManager.getSubscription().catch(() => null);
      if (subscription && key && !sameKey(subscription.options.applicationServerKey, key)) {
        // The PC's key changed: the old address goes nowhere now.
        handedOver = false;
        subscription = permissionNow() === "granted" ? await subscribe(registration, key).catch(() => null) : null;
      }
      if (subscription && !handedOver) {
        handedOver = await client
          .pushSubscribe(subscription.toJSON(), locale)
          .then(() => true)
          .catch(() => false);
      }
      if (!cancelled) setSubscribed(subscription !== null);
    })();
    // Allowed or blocked in the phone's settings meanwhile.
    const recheck = () => setPermission(permissionNow());
    document.addEventListener("visibilitychange", recheck);
    return () => {
      cancelled = true;
      document.removeEventListener("visibilitychange", recheck);
    };
  }, [client, support]);

  /** From the tap itself: the browser's prompt comes first, before any wait. */
  const turnOn = useCallback(() => {
    setError(null);
    setBusy(true);
    const { registration, key } = prepared.current;
    const subscribing =
      registration && key
        ? subscribe(registration, key)
        : (async () => {
            const worker = registration ?? (await workerRegistration());
            if (!worker) throw new Error("no-worker");
            return subscribe(worker, key ?? (await client.pushKey()).key);
          })();
    void subscribing
      .then(async (subscription) => {
        await client.pushSubscribe(subscription.toJSON(), locale);
        handedOver = true;
        setSubscribed(true);
      })
      .catch((cause: unknown) => {
        if (permissionNow() === "denied" || refusedByPerson(cause)) return;
        setError(t("push.failed", { reason: why(cause) }));
      })
      .finally(() => {
        setBusy(false);
        setPermission(permissionNow());
      });
  }, [client]);

  /** The browser forgets the subscription first: the PC is told, and would hear it was gone anyway. */
  const turnOff = useCallback(async () => {
    setError(null);
    setBusy(true);
    try {
      const subscription = await prepared.current.registration?.pushManager.getSubscription();
      await subscription?.unsubscribe();
      await client.pushUnsubscribe().catch(() => undefined);
      setSubscribed(false);
    } catch (cause) {
      setError(t("push.failed", { reason: why(cause) }));
    } finally {
      setBusy(false);
    }
  }, [client]);

  const status: PushStatus = noWorker ? "unsupported" : pushStatus(support, permission, subscribed);
  return { status, busy, error, turnOn, turnOff };
}

const SUB: Record<Exclude<PushStatus, "checking">, Key> = {
  on: "push.on",
  off: "push.off",
  denied: "push.blocked",
  install: "push.needsInstall",
  oldIos: "push.needsUpdate",
  unsupported: "push.notHere",
};

const EXPLAIN: Partial<Record<PushStatus, Key>> = {
  install: "push.install",
  oldIos: "push.oldIos",
  unsupported: "push.unsupported",
  denied: "push.denied",
};

/**
 * On For you: the offer to turn notifications on, until the person does or
 * says not now; then one row that says where they stand, with the switch.
 * On an iPhone in Safari, or a browser without them, the button explains
 * what it takes instead. For you shows the offer at its top and the row with
 * the others at its foot, from one `useNotifications`.
 */
export function useNotifications(): NotificationsViewProps {
  const { client } = useRemoteContext();
  const { status, busy, error, turnOn, turnOff } = usePush(client);
  const [dismissed, setDismissed] = useState(() => load("pushOfferDismissed") === "1");
  const [explaining, setExplaining] = useState(false);
  return {
    status,
    busy,
    error,
    dismissed,
    explaining,
    onTurnOn: turnOn,
    onTurnOff: () => void turnOff(),
    onExplain: setExplaining,
    onNotNow: () => {
      save("pushOfferDismissed", "1");
      setDismissed(true);
      setExplaining(false);
    },
  };
}

export interface NotificationsViewProps {
  status: PushStatus;
  busy: boolean;
  error: string | null;
  /** The offer was turned down: the row only. */
  dismissed: boolean;
  /** What it takes, shown instead. */
  explaining: boolean;
  onTurnOn: () => void;
  onTurnOff: () => void;
  onExplain: (shown: boolean) => void;
  onNotNow: () => void;
}

const offered = ({ status, dismissed }: Pick<NotificationsViewProps, "status" | "dismissed">) =>
  !dismissed && status !== "on" && status !== "denied";

/** Where For you shows it: the offer at the top, the row with the others at the foot. */
export function pushPlace(props: Pick<NotificationsViewProps, "status" | "dismissed">): "top" | "foot" | null {
  if (props.status === "checking") return null;
  return offered(props) ? "top" : "foot";
}

export function NotificationsView(props: NotificationsViewProps) {
  const { status, busy, error, explaining } = props;
  if (status === "checking") return null;
  const explanation = EXPLAIN[status];
  const tap = () => (explanation ? props.onExplain(true) : props.onTurnOn());
  const failure = error && (
    <p className="push__error" role="alert">
      {error}
    </p>
  );
  const gotIt = (
    <button type="button" className="btn btn--quiet" onClick={() => props.onExplain(false)}>
      {t("install.dismiss")}
    </button>
  );

  if (offered(props)) {
    // What it takes, in place of the offer that asked.
    if (explaining && explanation) {
      return (
        <aside className="notice" aria-live="polite">
          <Icon name={status === "install" ? "share" : "bell"} />
          <p className="notice__title">{t(status === "install" ? "push.installTitle" : "push.row")}</p>
          <p className="notice__body">{t(explanation)}</p>
          <div className="notice__actions">{gotIt}</div>
        </aside>
      );
    }
    return (
      <>
        <aside className="notice">
          <Icon name="bell" />
          <p className="notice__title">{t("push.title")}</p>
          <p className="notice__body">{t("push.body")}</p>
          <div className="notice__actions">
            <button type="button" className="btn btn--primary" disabled={busy} onClick={tap}>
              {busy ? t("push.turningOn") : t("push.turnOn")}
            </button>
            <button type="button" className="btn btn--quiet" onClick={props.onNotNow}>
              {t("push.notNow")}
            </button>
          </div>
        </aside>
        {failure}
      </>
    );
  }

  const switchable = status === "off" || status === "install";
  const told = explaining && explanation;
  return (
    <>
      <div className="row row--push">
        <Icon name="bell" />
        <span className="row__main">
          <span className="row__title">{t("push.row")}</span>
          <span className="row__sub">{t(SUB[status])}</span>
        </span>
        {status === "on" ? (
          <button type="button" className="btn btn--quiet" disabled={busy} onClick={props.onTurnOff}>
            {t("push.turnOff")}
          </button>
        ) : switchable && !told ? (
          <button type="button" className="btn" disabled={busy} onClick={tap}>
            {busy ? t("push.turningOn") : t("push.turnOnShort")}
          </button>
        ) : explanation && !told ? (
          <button type="button" className="btn btn--quiet" onClick={() => props.onExplain(true)}>
            {t("push.why")}
          </button>
        ) : null}
      </div>
      {told && (
        // What it takes, under the row that asked.
        <div className="row__more" aria-live="polite">
          <p>{t(explanation)}</p>
          {gotIt}
        </div>
      )}
      {failure}
    </>
  );
}
