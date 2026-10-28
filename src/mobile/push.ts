import { isIos, isStandalone } from "./device";

/**
 * Push notifications on the phone: whether this browser can have them, and
 * what turning them on takes. The PC sends them itself
 * (src-tauri/src/remote/push.rs) and the service worker shows them (sw.js);
 * the switch the person sees is components/Notifications.tsx.
 *
 * An iPhone gives them only to an app on its Home Screen, from iOS 16.4, and
 * asks the person only on a tap: the subscription is the first thing a tap
 * does, with the worker and the PC's key read beforehand.
 */

/**
 * - ok: this browser can have them.
 * - install: an iPhone or iPad in Safari: Hangar must be on the Home Screen first.
 * - oldIos: on the Home Screen, but iOS older than 16.4.
 * - unsupported: a browser without push at all.
 */
export type PushSupport = "ok" | "install" | "oldIos" | "unsupported";

export interface Environment {
  ios: boolean;
  standalone: boolean;
  serviceWorker: boolean;
  pushManager: boolean;
  notification: boolean;
}

export function environment(): Environment {
  return {
    ios: isIos(navigator.userAgent, navigator.maxTouchPoints),
    standalone: isStandalone(),
    serviceWorker: "serviceWorker" in navigator,
    pushManager: typeof PushManager !== "undefined",
    notification: typeof Notification !== "undefined",
  };
}

export function pushSupport(env: Environment): PushSupport {
  if (env.ios && !env.standalone) return "install";
  if (env.serviceWorker && env.pushManager && env.notification) return "ok";
  return env.ios ? "oldIos" : "unsupported";
}

/** What the person sees of it. */
export type PushStatus = "checking" | "on" | "off" | "denied" | Exclude<PushSupport, "ok">;

export function pushStatus(
  support: PushSupport,
  permission: NotificationPermission | null,
  subscribed: boolean | null,
): PushStatus {
  if (support !== "ok") return support;
  if (permission === "denied") return "denied";
  if (subscribed === null) return "checking";
  return subscribed && permission === "granted" ? "on" : "off";
}

/** A base64url key as the bytes `applicationServerKey` takes. */
export function keyBytes(key: string): Uint8Array<ArrayBuffer> {
  const base64 = key.trim().replace(/-/g, "+").replace(/_/g, "/").replace(/=+$/, "");
  const raw = atob(base64.padEnd(Math.ceil(base64.length / 4) * 4, "="));
  return Uint8Array.from(raw, (char) => char.charCodeAt(0));
}

/** Whether a subscription was made with this key: one made with another goes nowhere. */
export function sameKey(made: ArrayBuffer | null | undefined, key: string): boolean {
  if (!made) return false;
  const a = new Uint8Array(made);
  const b = keyBytes(key);
  return a.length === b.length && a.every((byte, i) => byte === b[i]);
}

const options = (key: string): PushSubscriptionOptionsInit => ({
  userVisibleOnly: true,
  applicationServerKey: keyBytes(key),
});

/**
 * Subscribes with the PC's key. The browser's own call is made before
 * anything is awaited, so a tap's permission prompt still counts as the tap's.
 * A subscription left from another key is replaced.
 */
export async function subscribe(registration: ServiceWorkerRegistration, key: string): Promise<PushSubscription> {
  try {
    return await registration.pushManager.subscribe(options(key));
  } catch (error) {
    const old = await registration.pushManager.getSubscription();
    if (!old || sameKey(old.options.applicationServerKey, key)) throw error;
    await old.unsubscribe();
    return registration.pushManager.subscribe(options(key));
  }
}

const READY_MS = 4_000;

/** The service worker once it runs; null without one (the dev server registers none). */
export function workerRegistration(): Promise<ServiceWorkerRegistration | null> {
  if (!("serviceWorker" in navigator)) return Promise.resolve(null);
  return Promise.race([
    navigator.serviceWorker.ready,
    new Promise<null>((resolve) => setTimeout(() => resolve(null), READY_MS)),
  ]);
}

/** A tap the browser turned down: the person said no, or closed the prompt. */
export const refusedByPerson = (error: unknown) => error instanceof DOMException && error.name === "NotAllowedError";
