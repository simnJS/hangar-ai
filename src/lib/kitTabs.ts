import { useEffect, useSyncExternalStore } from "react";
import { emit, listen } from "@tauri-apps/api/event";
import tokenMap from "../../src-tauri/kit-sdk/tokens.json";
import type { Locale } from "../i18n";
import { deriveChromeTokens, type TerminalTheme } from "../themes";
import {
  isKitPermission,
  kitCommandName,
  kitCommandsGrant,
  kitPermissionsIn,
  kitsList,
  onKitsChanged,
  workspaceKits,
  type KitCommandsGrant,
  type KitInfo,
  type KitPermission,
  type KitTab,
} from "./kits";
import { kitRequested, kitState } from "./kitActivation";
import type { KitTabParams } from "./modelStore";
import { UI_SCALE_MAX, UI_SCALE_MIN, type Workspace } from "../types";

/**
 * A kit's tabs, as a workspace offers them (docs/kits.md): which tabs its
 * enabled kits have, the URL each page is served at by the `hangar-kit`
 * scheme (src-tauri/src/kit_scheme.rs), what each may ask Hangar for, and what
 * a caller may pass it. The page itself is drawn by
 * components/preview/KitTabView, and answered by lib/kitApi.
 */

/** What a caller may pass a tab under one name (`Tab::params` in kits/manifest.rs). */
export interface KitTabParam {
  description: string;
  /** A value must match it whole. */
  pattern?: string;
}

declare module "./kits" {
  interface KitTab {
    /** What a caller may pass it, by name. */
    params?: Record<string, KitTabParam>;
    /** One tab whatever its params, which new ones replace. */
    singleton?: boolean;
  }
}

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
  /**
   * The `commands:<hash>` this tab's commands run under, which Rust compares
   * with the kit as it is at each call. Null for a kit that is not asked: a
   * linked folder, or no command.
   */
  commandsAccepted: KitCommandsGrant | null;
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
    // A GitHub kit's commands run only as the workspace accepted them: changed,
    // its tabs hold none until they are accepted again, whatever else they hold.
    const commandsGrant = kitCommandsGrant(kit);
    const commandsHeld = !commandsGrant || granted.includes(commandsGrant);
    return kit.manifest.tabs.map((tab) => ({
      kitId: id,
      kitName,
      tab,
      permissions: effectivePermissions(tab.permissions.filter(isKitPermission), granted).filter(
        (permission) => commandsHeld || kitCommandName(permission) === null,
      ),
      commandsAccepted: commandsHeld ? commandsGrant : null,
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

/**
 * Whether a kit tab's page is loaded now. A tab brought back at startup waits
 * for the first time it is shown; one an agent or a hook opened, or asked for
 * again, loads at once, as it may be following something (a CI run). Once
 * shown, a tab is a normal one: its page is kept loaded, hidden or not.
 */
export const kitTabLoads = (tab: { restored: boolean }, shown: boolean): boolean => !tab.restored || shown;


/* ── what an agent or a hook may open ─────────────────────────── */

/** The longest value a param may hold, in characters. */
export const KIT_TAB_PARAM_MAX_CHARS = 1024;

/** A value's rule as this window reads it, anchored; null for one JavaScript cannot read. */
function paramRule(pattern: string): RegExp | null {
  // Alone first: `a)|(?:b` only parses once wrapped, and would then match past
  // the anchors. As Unicode first, for `\p{L}`, then without, for the stray
  // escapes it refuses and Rust's syntax allows.
  for (const flags of ["u", ""]) {
    try {
      new RegExp(pattern, flags);
      return new RegExp(`^(?:${pattern})$`, flags);
    } catch {
      /* next */
    }
  }
  return null;
}

/**
 * The params a caller gave, held to what the tab declares: known names, strings,
 * each matching its rule whole. Rust held them to the same rules first, where a
 * rule cannot backtrack for ever (`Tab::param_refusal`). The message is the
 * caller's to act on.
 */
export function readKitTabParams(tab: KitTab, raw: unknown): KitTabParams {
  if (raw === undefined || raw === null) return {};
  if (typeof raw !== "object" || Array.isArray(raw)) {
    throw new Error("params must be an object of strings, by the names the tab declares.");
  }
  const declared = tab.params ?? {};
  const names = Object.keys(declared);
  const taken = names.length ? `It takes: ${names.join(", ")}.` : "It takes none.";
  const entries = Object.entries(raw as Record<string, unknown>);
  for (const [name, value] of entries) {
    if (!Object.prototype.hasOwnProperty.call(declared, name)) {
      throw new Error(`The tab "${tab.id}" declares no param ${JSON.stringify(name)}. ${taken}`);
    }
    if (typeof value !== "string") throw new Error(`Param "${name}" must be a string.`);
    if (value.length > KIT_TAB_PARAM_MAX_CHARS) {
      throw new Error(`Param "${name}" must be at most ${KIT_TAB_PARAM_MAX_CHARS} characters.`);
    }
    const { pattern } = declared[name];
    if (pattern === undefined) continue;
    const rule = paramRule(pattern);
    if (!rule) {
      throw new Error(
        `Param "${name}" cannot be checked: Hangar's window cannot read the kit's rule ${pattern}, so nothing passes. ` +
          "The kit has to write it in the syntax Rust and JavaScript share.",
      );
    }
    if (!rule.test(value)) {
      throw new Error(`Param "${name}" must match its rule ${pattern}, and ${JSON.stringify(value.slice(0, 60))} does not.`);
    }
  }
  return Object.fromEntries(entries) as KitTabParams;
}

/**
 * The kit and its tab, for an agent or a hook opening it on the workspace; the
 * reason when they may not, said so it can be acted on. What the person opens
 * from the palette or the menu does not go through here.
 */
export function kitTabToOpen(
  ws: Pick<Workspace, "kits" | "name">,
  installed: readonly KitInfo[],
  kitId: string,
  tabId: string,
): { kit: KitInfo; tab: KitTab } {
  const enabled = workspaceKits(ws);
  const entry = enabled.find((kit) => kit.id === kitId);
  if (!entry) {
    const list = enabled.map((kit) => kit.id).join(", ");
    throw new Error(
      `The kit "${kitId}" is not enabled on "${ws.name}". ${list ? `Enabled there: ${list}.` : "No kit is enabled there."} ` +
        "Only the person can enable one.",
    );
  }
  const kit = installed.find((candidate) => candidate.id === kitId);
  if (!kit) throw new Error(`The kit "${kitId}" is enabled on "${ws.name}", and no longer installed.`);
  const manifest = usable(installed, kitId)?.manifest;
  if (!manifest) throw new Error(`The kit "${kitId}" cannot be used right now: ${kit.error ?? "its manifest is unreadable"}.`);
  if (kitState(entry, kitRequested(kit)) === "review") {
    throw new Error(
      `The kit "${kitId}" has changes to review on "${ws.name}": nothing of it can be opened until the person accepts them, ` +
        "in the workspace's Kits dialog or in Settings › Kits. Tell them.",
    );
  }
  const tab = manifest.tabs.find((candidate) => candidate.id === tabId);
  if (!tab) {
    const tabs = manifest.tabs.map((candidate) => candidate.id).join(", ");
    throw new Error(`The kit "${kitId}" has no tab "${tabId}". ${tabs ? `Its tabs: ${tabs}.` : "It has no tabs."}`);
  }
  return { kit, tab };
}

/** What `kit_tab_list` says of the tabs a workspace's kits bring. */
export function kitTabListing(ws: Pick<Workspace, "kits">, installed: readonly KitInfo[]) {
  const enabled = workspaceKits(ws);
  return workspaceKitTabs(ws, installed).map(({ kitId, kitName, tab }) => {
    const kit = installed.find((candidate) => candidate.id === kitId)!;
    const entry = enabled.find((candidate) => candidate.id === kitId);
    return {
      kit: kitId,
      kitName,
      tab: tab.id,
      title: tab.title,
      description: kit.manifest?.description ?? "",
      singleton: Boolean(tab.singleton),
      params: tab.params ?? {},
      ...(kitState(entry, kitRequested(kit)) === "review" ? { toReview: true } : {}),
    };
  });
}

/**
 * Whether a page took the keyboard from a terminal by itself: focus went into its
 * frame from a terminal, and no pointer is over the frame, so nobody clicked it.
 * A terminal keeps Tab, so the keyboard cannot have walked there either.
 */
export const frameTookKeyboard = (focus: { inFrame: boolean; pointerOver: boolean; fromTerminal: boolean }) =>
  focus.inFrame && !focus.pointerOver && focus.fromTerminal;

/* ── the theme a tab is told (docs/kit-ui.md) ─────────────────── */

/** The `theme` event of the tab message protocol. */
export interface KitThemeEvent {
  hangar: 1;
  event: "theme";
  data: {
    scheme: TerminalTheme["appearance"];
    locale: Locale;
    tokens: Record<string, string>;
  };
}

/** The interface size as the window applies it (App.tsx): 1 at 100 %. */
export const kitUiScale = (percent: number): number =>
  Math.min(UI_SCALE_MAX, Math.max(UI_SCALE_MIN, percent || 100)) / 100;

/**
 * What a tab hears of its workspace's theme: any theme, bundled or not, since
 * every colour comes from deriveChromeTokens, which holds text to the contrast
 * it promises. The map is imported rather than copied, so a token added to
 * tokens.json reaches the page by itself.
 */
export function kitThemeEvent(
  theme: TerminalTheme,
  settings: { uiScale: number; mono: string; locale: Locale },
): KitThemeEvent {
  const chrome = deriveChromeTokens(theme);
  const tokens: Record<string, string> = {
    "--kit-scale": String(settings.uiScale),
    "--kit-mono": settings.mono,
  };
  for (const [token, source] of Object.entries(tokenMap.colors)) tokens[token] = chrome[source];
  return { hangar: 1, event: "theme", data: { scheme: theme.appearance, locale: settings.locale, tokens } };
}

/**
 * Tells one frame its theme: when its page has loaded, and at each change from
 * then on. Before the load there is nobody to hear it, so only the latest is
 * kept. Target origin `*`, as for every event: the frame's origin is opaque.
 */
export function kitThemeSender(frame: () => Pick<Window, "postMessage"> | null) {
  let loaded = false;
  let latest: KitThemeEvent | null = null;
  const send = () => {
    if (loaded && latest) frame()?.postMessage(latest, "*");
  };
  return {
    load() {
      loaded = true;
      send();
    },
    set(event: KitThemeEvent) {
      latest = event;
      send();
    },
  };
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
