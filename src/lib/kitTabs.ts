import { useEffect, useSyncExternalStore } from "react";
import { emit, listen } from "@tauri-apps/api/event";
import {
  isKitPermission,
  kitPermissionsIn,
  kitsList,
  onKitsChanged,
  workspaceKits,
  type KitInfo,
  type KitPermission,
  type KitTab,
} from "./kits";
import type { Workspace } from "../types";

/**
 * A kit's tabs, as a workspace offers them (docs/kits.md): which tabs its
 * enabled kits have, the URL each page is served at by the `hangar-kit`
 * scheme (src-tauri/src/kit_scheme.rs), and what each may ask Hangar for.
 * The page itself is drawn by components/preview/KitTabView, and answered
 * by lib/kitApi.
 */

const IS_WINDOWS = typeof navigator !== "undefined" && /windows/i.test(navigator.userAgent);

/**
 * Where the scheme serves a file of a kit: Windows' webview reaches custom
 * schemes as `http://<scheme>.localhost`, the others by the scheme itself.
 * Segments are encoded one by one, so the page's relative URLs resolve
 * inside the kit.
 */
export function kitFileUrl(kitId: string, path: string, platformIsWindows = IS_WINDOWS): string {
  const base = platformIsWindows ? "http://hangar-kit.localhost" : "hangar-kit://localhost";
  const parts = path
    .split(/[\\/]/)
    .filter((part) => part && part !== ".")
    .map((part) => encodeURIComponent(part));
  return `${base}/${encodeURIComponent(kitId)}/${parts.join("/")}`;
}

/**
 * What a tab may call: what its manifest asks for and the person granted
 * the kit on the workspace, both. A permission granted that the tab does not
 * declare stays out of it, and one it declares that was not granted — added
 * by an update since — waits for the person to grant it.
 */
export function effectivePermissions(
  asked: readonly string[],
  granted: readonly string[],
): KitPermission[] {
  return kitPermissionsIn(asked.filter((permission) => granted.includes(permission)));
}

/** A tab the workspace can open: its kit enabled there, installed and usable. */
export interface KitTabChoice {
  kitId: string;
  kitName: string;
  tab: KitTab;
  permissions: KitPermission[];
}

/** An installed kit that can be used now, under the id it is enabled by. */
function usable(installed: readonly KitInfo[], id: string): KitInfo | null {
  const kit = installed.find((entry) => entry.id === id);
  return kit && !kit.error && kit.manifest && kit.manifest.id === id ? kit : null;
}

/** Every tab of the workspace's enabled kits, in the order the kits were enabled. */
export function workspaceKitTabs(
  ws: Pick<Workspace, "kits"> | null | undefined,
  installed: readonly KitInfo[] | null,
): KitTabChoice[] {
  if (!installed) return [];
  return workspaceKits(ws).flatMap(({ id, granted }) => {
    const kit = usable(installed, id);
    if (!kit?.manifest) return [];
    const kitName = kit.manifest.name;
    return kit.manifest.tabs.map((tab) => ({
      kitId: id,
      kitName,
      tab,
      permissions: effectivePermissions(tab.permissions.filter(isKitPermission), granted),
    }));
  });
}

/** What a kit tab of the preview runs now, or why it does not run. */
export type KitTabState =
  | { kind: "ready"; choice: KitTabChoice; url: string }
  /** The installed kits are being read. */
  | { kind: "loading" }
  /** The kit is not enabled on the workspace any more. */
  | { kind: "disabled" }
  /** The kit is not installed any more, or has no such tab now. */
  | { kind: "missing" }
  /** Installed, and unusable as it stands. */
  | { kind: "broken"; error: string };

export function kitTabState(
  ws: Pick<Workspace, "kits"> | null | undefined,
  installed: readonly KitInfo[] | null,
  kitId: string,
  tabId: string,
  platformIsWindows = IS_WINDOWS,
): KitTabState {
  if (!workspaceKits(ws).some((kit) => kit.id === kitId)) return { kind: "disabled" };
  if (!installed) return { kind: "loading" };
  const kit = installed.find((entry) => entry.id === kitId);
  if (!kit) return { kind: "missing" };
  if (!usable(installed, kitId)) return { kind: "broken", error: kit.error ?? "" };
  const choice = workspaceKitTabs(ws, installed).find(
    (entry) => entry.kitId === kitId && entry.tab.id === tabId,
  );
  if (!choice) return { kind: "missing" };
  return { kind: "ready", choice, url: kitFileUrl(kitId, choice.tab.entry, platformIsWindows) };
}

/* ── the installed kits, read once for the window ─────────────── */

let installed: KitInfo[] | null = null;
let watching = false;
let reading: Promise<void> | null = null;
const listeners = new Set<() => void>();

function changed(next: KitInfo[]) {
  installed = next;
  for (const listener of listeners) listener();
}

function read(): Promise<void> {
  // A read that lands after a newer one started is dropped by the newer one.
  const mine = kitsList()
    .then((kits) => {
      if (reading === mine) changed(kits);
    })
    // Outside the desktop shell, or an index that cannot be read: no kit.
    .catch(() => {
      if (reading === mine) changed([]);
    });
  reading = mine;
  return mine;
}

function watch() {
  if (watching) return;
  watching = true;
  void read();
  onKitsChanged(() => void read()).catch(() => undefined);
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/**
 * The kits installed on this machine, kept current by `kits:changed`; null
 * until first read.
 */
export function useInstalledKits(): KitInfo[] | null {
  useEffect(watch, []);
  return useSyncExternalStore(subscribe, () => installed);
}

/* ── the preview's folders, granted by the main window ─────────── */

/**
 * Rust serves the preview from one list of folders for the whole app, which
 * the main window keeps (lib/modelHost › grantModelRoots): a workspace
 * window granting folders itself would replace the main window's list with
 * its own. So a kit tab opening a file asks the main window to grant its
 * workspace's folders, the same ones an agent's preview_show grants.
 */
const GRANT_ASK = "kits:grant-roots";
const GRANT_DONE = "kits:grant-roots-done";
const GRANT_MS = 5000;

interface GrantAsk {
  reqId: string;
  workspaceId: string;
}

interface GrantDone {
  reqId: string;
  ok: boolean;
  error?: string;
}

let grants = 0;

/** From a workspace window: has the main window grant the workspace's folders. */
export function askRootGrant(workspaceId: string): Promise<void> {
  const reqId = `grant:${Date.now().toString(36)}:${++grants}`;
  return new Promise((resolve, reject) => {
    let off: (() => void) | null = null;
    let settled = false;
    const finish = (fn: () => void) => {
      if (settled) return;
      settled = true;
      window.clearTimeout(timer);
      off?.();
      fn();
    };
    const timer = window.setTimeout(
      () => finish(() => reject(new Error("the main window did not answer"))),
      GRANT_MS,
    );
    listen<GrantDone>(GRANT_DONE, (event) => {
      if (event.payload.reqId !== reqId) return;
      const { ok, error } = event.payload;
      finish(() => (ok ? resolve() : reject(new Error(error ?? "the folders could not be granted"))));
    })
      .then((stop) => {
        if (settled) return stop();
        off = stop;
        return emit(GRANT_ASK, { reqId, workspaceId } satisfies GrantAsk);
      })
      .catch((err) => finish(() => reject(err instanceof Error ? err : new Error(String(err)))));
  });
}

/** The main window: grants what workspace windows ask for. Returns the unsubscribe. */
export function serveRootGrants(grant: (workspaceId: string) => Promise<void>): () => void {
  let off: (() => void) | null = null;
  let stopped = false;
  listen<GrantAsk>(GRANT_ASK, (event) => {
    const { reqId, workspaceId } = event.payload;
    grant(workspaceId)
      .then(() => emit(GRANT_DONE, { reqId, ok: true } satisfies GrantDone))
      .catch((err: unknown) =>
        emit(GRANT_DONE, {
          reqId,
          ok: false,
          error: err instanceof Error ? err.message : String(err),
        } satisfies GrantDone),
      )
      .catch(() => undefined);
  })
    .then((stop) => {
      if (stopped) stop();
      else off = stop;
    })
    .catch(() => undefined);
  return () => {
    stopped = true;
    off?.();
  };
}
