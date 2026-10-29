import { ApiError, type App } from "./api";
import { errorText } from "./errors";
import type { Translator } from "./i18n";

/**
 * The apps the agents build, seen from the phone: which a project or a
 * worker offers, what its chip says, and the viewer from the tap to the app
 * on screen, or why it is not.
 *
 * The PC lists only the dev servers that listen (remote_apps): a worker's,
 * found in its port block, and the ports the person added to the project in
 * Settings › Mobile. A tap opens one through a slot of the PC's own, an
 * address of the tailnet with the app at its root (/m/api/app/open).
 */

/** A project's apps, in the PC's order: its workers' first, then the person's. */
export const appsOf = (apps: App[] | null, projectId: string): App[] =>
  (apps ?? []).filter((app) => app.workspaceId === projectId);

/** A worker's apps, by its pane's name (names are unique across Hangar). */
export const workerApps = (apps: App[] | null, projectId: string, pane: string): App[] =>
  appsOf(apps, projectId).filter((app) => app.source === "worker" && app.pane === pane);

/**
 * What a project's card or a worker's header offers: the app itself when
 * there is one, the project's list when there are several, nothing without.
 */
export type AppChip = { kind: "one"; app: App } | { kind: "many"; count: number } | null;

export function appChip(apps: App[]): AppChip {
  if (apps.length === 0) return null;
  return apps.length === 1 ? { kind: "one", app: apps[0] } : { kind: "many", count: apps.length };
}

/** Who the app is: its worker, or the person's own server. */
export function appName(app: App, { t }: Translator): string {
  return app.source === "worker" && app.pane ? app.pane : t("apps.custom");
}

/** Under its name: the worker's task, or where the port was added. */
export function appDetail(app: App, { t }: Translator): string {
  if (app.source === "custom") return t("apps.customHint");
  return app.task ?? t("apps.workerHint");
}

/* ── The viewer ────────────────────────────────────────────────── */

/**
 * - starting: the PC is asked for a slot (the first time, Tailscale Serve
 *   is told about it, which takes seconds);
 * - loading: the app's address is known, its page has not come yet;
 * - shown: the app is on screen;
 * - stopped: nothing listens on its port any more;
 * - refused: the PC will not open it, and says why;
 * - failed: the PC could not be asked, or was too busy to answer.
 */
export type Viewer =
  | { kind: "starting" }
  | { kind: "loading"; url: string }
  | { kind: "shown"; url: string }
  | { kind: "stopped" }
  | { kind: "refused"; reason: string }
  | { kind: "failed"; reason: string };

/**
 * What the viewer says when the PC did not open the app, from its answer.
 * The PC offers only the ports a server listens on, so an app the phone has
 * seen listed that it no longer offers (`not-allowed`) has stopped, as one
 * that stopped while being opened (`not-listening`); `known` says whether
 * the phone has.
 */
export function openFailure(error: unknown, port: number, { t }: Translator, known = false): Viewer {
  if (error instanceof ApiError) {
    switch (error.code) {
      case "not-listening":
        return { kind: "stopped" };
      case "not-allowed":
        return known ? { kind: "stopped" } : { kind: "refused", reason: t("app.notAllowed", { port }) };
      case "no-free-slot":
        return { kind: "refused", reason: t("app.noSlot") };
      case "tailscale":
        return { kind: "refused", reason: t("app.tailscale", { reason: error.detail ?? error.code }) };
    }
    if (error.kind === "refused") return { kind: "refused", reason: error.detail ?? error.code };
  }
  return { kind: "failed", reason: errorText(error) };
}

/**
 * Listings without the app before the viewer says its server stopped: a dev
 * server restarting (a config change, a crash Vite recovers from) misses one.
 */
export const MISSES_BEFORE_STOPPED = 2;

/**
 * The viewer after a listing of the project's apps: an app on screen, or on
 * its way, whose port the PC no longer lists `MISSES_BEFORE_STOPPED` times
 * in a row has stopped. `misses` counts them; any other state is left as it is.
 */
export function afterListing(viewer: Viewer, listed: boolean, misses: number): { viewer: Viewer; misses: number } {
  if (viewer.kind !== "shown" && viewer.kind !== "loading") return { viewer, misses: 0 };
  if (listed) return { viewer, misses: 0 };
  const missed = misses + 1;
  return missed >= MISSES_BEFORE_STOPPED ? { viewer: { kind: "stopped" }, misses: 0 } : { viewer, misses: missed };
}
