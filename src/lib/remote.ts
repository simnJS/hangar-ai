/**
 * Hangar Mobile, from the settings: typed bindings over the Rust commands
 * (src-tauri/src/remote) and the little logic the section needs — which
 * Tailscale step the person is on, the pairing countdown, a device's dates.
 */

import { invoke } from "@tauri-apps/api/core";

/** Mirrors `RemoteStatus`. */
export interface RemoteStatus {
  enabled: boolean;
  running: boolean;
  port: number;
  /** Why the server could not start: the port taken, most likely. */
  error: string | null;
}

/** Mirrors `PairStart`. */
export interface PairStart {
  code: string;
  /** What the QR code holds. */
  url: string;
  qrSvg: string;
  /** Milliseconds since the epoch. */
  expiresAt: number;
}

/** Mirrors `DeviceView`; the dates are milliseconds since the epoch. */
export interface RemoteDevice {
  id: string;
  name: string;
  created: number;
  /** 0 until the phone has made a call. */
  lastSeen: number;
}

/** Mirrors `TailscaleStatus`. */
export interface TailscaleStatus {
  installed: boolean;
  running: boolean;
  dnsName: string | null;
  serving: boolean;
}

/**
 * Mirrors `SlotView` (src-tauri/src/remote/apps.rs): one of the six ports an
 * app shown on the phone goes through, HTTPS on the tailnet to a listener of
 * Hangar's on 127.0.0.1.
 */
export interface AppSlot {
  slot: number;
  /** 8443 to 8448: `https://<pc>.<tailnet>.ts.net:<httpsPort>/`. */
  httpsPort: number;
  /** 47822 to 47827: Hangar's listener, which Tailscale Serve proxies to. */
  localPort: number;
  /**
   * What Tailscale Serve does with `httpsPort`: mapped to the slot, free, or
   * taken by something of the person's, which Hangar leaves alone. Null when
   * Tailscale could not be asked.
   */
  mapping: "mapped" | "free" | "taken" | null;
  /** The app it shows, while it does; freed 30 minutes after `lastUsed`. */
  app: { workspaceId: string; workspace: string; port: number; lastUsed: number } | null;
}

/** The event the PC emits when a phone is paired or revoked. */
export const REMOTE_CHANGED = "remote:changed";

export const TAILSCALE_DOWNLOAD_URL = "https://tailscale.com/download";

export const remoteStatus = () => invoke<RemoteStatus>("remote_status");
export const remoteSetEnabled = (enabled: boolean) =>
  invoke<RemoteStatus>("remote_set_enabled", { enabled });
export const remotePairStart = () => invoke<PairStart>("remote_pair_start");
export const remoteDevices = () => invoke<RemoteDevice[]>("remote_devices");
export const remoteRevoke = (id: string) => invoke<void>("remote_revoke", { id });
export const remoteTailscale = () => invoke<TailscaleStatus>("remote_tailscale");
/** Runs `tailscale serve`: only ever from the person's click. */
export const remoteTailscaleServe = () => invoke<TailscaleStatus>("remote_tailscale_serve");

export const remoteAppSlots = () => invoke<AppSlot[]>("remote_app_slots");

/* ── The apps the phone may open (lib/remoteApps) ───────────────── */

/** Below 1024 a port is the system's, past 65535 there is none. */
export const APP_PORT_MIN = 1024;
export const APP_PORT_MAX = 65535;

/**
 * The slots' listeners, until the PC has listed them: beside the mobile
 * server's 47821, as apps.rs fixes them.
 */
export const DEFAULT_SLOT_PORTS: readonly number[] = [47822, 47823, 47824, 47825, 47826, 47827];

const isAppPort = (port: unknown): port is number =>
  typeof port === "number" && Number.isInteger(port) && port >= APP_PORT_MIN && port <= APP_PORT_MAX;

/**
 * The ports the person added to a workspace, as saved: whole ports in range,
 * each once, in order. Anything else in the file is left out.
 */
export function appPortsOf(ws: { mobilePorts?: unknown }): number[] {
  const saved = Array.isArray(ws.mobilePorts) ? ws.mobilePorts : [];
  return [...new Set(saved.filter(isAppPort))].sort((a, b) => a - b);
}

/**
 * Hangar's own ports, which no app may be: the mobile server's, and each
 * slot's listener (what Tailscale Serve proxies to), which the mobile server
 * refuses to show anyway.
 */
export function hangarPorts(remote: RemoteStatus | null, slots: AppSlot[] | null): number[] {
  const own = new Set(slots?.length ? slots.map((slot) => slot.localPort) : DEFAULT_SLOT_PORTS);
  if (remote) own.add(remote.port);
  return [...own].sort((a, b) => a - b);
}

/** Why a port cannot be added: not a number, out of range, Hangar's own, already there. */
export type AppPortProblem = "invalid" | "range" | "hangar" | "duplicate";

/** The port typed, if the person may add it to `list`; why not otherwise. */
export function checkAppPort(
  text: string,
  list: readonly number[],
  reserved: readonly number[],
): { port: number } | { problem: AppPortProblem } {
  const typed = text.trim();
  if (!/^\d{1,6}$/.test(typed)) return { problem: "invalid" };
  const port = Number(typed);
  if (!isAppPort(port)) return { problem: "range" };
  if (reserved.includes(port)) return { problem: "hangar" };
  if (list.includes(port)) return { problem: "duplicate" };
  return { port };
}

export const withAppPort = (list: readonly number[], port: number): number[] =>
  [...new Set([...list, port])].sort((a, b) => a - b);

export const withoutAppPort = (list: readonly number[], port: number): number[] =>
  list.filter((entry) => entry !== port);

/** Where the person is in setting Tailscale up, one step at a time. */
export type TailscaleStep = "missing" | "stopped" | "unserved" | "ready";

export function tailscaleStep(status: TailscaleStatus): TailscaleStep {
  if (!status.installed) return "missing";
  // Connected without a name on the tailnet is not an address to hand out.
  if (!status.running || !status.dnsName) return "stopped";
  return status.serving ? "ready" : "unserved";
}

/** The address the phone opens, once Tailscale has a name for the PC. */
export function remoteAddress(status: TailscaleStatus): string | null {
  return tailscaleStep(status) === "missing" || !status.dnsName
    ? null
    : `https://${status.dnsName}`;
}

/** Phones can only be paired once the server runs and the address is served. */
export function canPair(remote: RemoteStatus | null, tailscale: TailscaleStatus | null): boolean {
  return !!remote?.enabled && remote.running && !!tailscale && tailscaleStep(tailscale) === "ready";
}

/** Whole seconds left before `expiresAt`, never negative. */
export function secondsLeft(expiresAt: number, now: number): number {
  return Math.max(0, Math.ceil((expiresAt - now) / 1000));
}

/** `m:ss`, the way a countdown reads. */
export function formatCountdown(seconds: number): string {
  const whole = Math.max(0, Math.floor(seconds));
  return `${Math.floor(whole / 60)}:${String(whole % 60).padStart(2, "0")}`;
}

/** Phones the list did not hold before: the pairing that was waiting went through. */
export function newDevices(before: ReadonlySet<string>, after: RemoteDevice[]): RemoteDevice[] {
  return after.filter((device) => !before.has(device.id));
}

/** Most recently paired first. */
export function sortDevices(devices: RemoteDevice[]): RemoteDevice[] {
  return [...devices].sort((a, b) => b.created - a.created);
}

/** "Oct 28, 8:41 PM" / "28 oct., 20:41"; the year only when it is not this one. */
export function formatStamp(ms: number, locale: string, now: number = Date.now()): string {
  const date = new Date(ms);
  const sameYear = date.getFullYear() === new Date(now).getFullYear();
  return new Intl.DateTimeFormat(locale, {
    day: "numeric",
    month: "short",
    ...(sameYear ? {} : { year: "numeric" }),
    hour: "2-digit",
    minute: "2-digit",
  }).format(date);
}
