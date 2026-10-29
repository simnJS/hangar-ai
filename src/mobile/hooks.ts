import { useCallback, useEffect, useRef, useState } from "react";
import { startPolling } from "./poll";
import { parseRoute, routeHash, type Route } from "./route";
import { browserPage, watchViewport } from "./viewport";

/** Runs `task` every `intervalMs` while the page is seen and `enabled`. */
export function usePolling(task: () => Promise<unknown>, intervalMs: number, enabled = true): void {
  const latest = useRef(task);
  latest.current = task;
  useEffect(() => {
    if (!enabled) return;
    return startPolling(() => latest.current(), intervalMs);
  }, [intervalMs, enabled]);
}

/** The current time, ticking every `intervalMs`, for "for 4 min" and the like. */
export function useNow(intervalMs = 15_000): number {
  const [now, setNow] = useState(Date.now);
  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), intervalMs);
    return () => clearInterval(timer);
  }, [intervalMs]);
  return now;
}

/**
 * The screen shown, from the address's hash. Moving on pushes a history
 * entry, so the phone's back gesture returns; `back` goes to the entry before
 * when the app made it, and to `fallback` when the app was opened right here
 * (from a notification, say).
 */
export function useRoute() {
  const [route, setRoute] = useState<Route>(() => parseRoute(location.hash));

  useEffect(() => {
    const update = () => setRoute(parseRoute(location.hash));
    addEventListener("popstate", update);
    addEventListener("hashchange", update);
    return () => {
      removeEventListener("popstate", update);
      removeEventListener("hashchange", update);
    };
  }, []);

  const navigate = useCallback((next: Route, options: { replace?: boolean } = {}) => {
    const depth = (history.state?.depth ?? 0) + (options.replace ? 0 : 1);
    const url = `${location.pathname}${location.search}${routeHash(next)}`;
    if (options.replace) history.replaceState({ depth }, "", url);
    else history.pushState({ depth }, "", url);
    setRoute(next);
  }, []);

  const back = useCallback(
    (fallback: Route) => {
      if ((history.state?.depth ?? 0) > 0) history.back();
      else navigate(fallback, { replace: true });
    },
    [navigate],
  );

  return { route, navigate, back };
}

/** The screen a notification's address names, as its hash: `#/pane/p1`, `#/shares`, `#/`. */
export function hashOf(url: string, base: string): string {
  try {
    return new URL(url, base).hash || "#/";
  } catch {
    return "#/";
  }
}

/**
 * A notification tapped while the app is open: the service worker brings it
 * forward and says which screen (sw.js), and the app goes there.
 */
export function useNotificationOpens(): void {
  useEffect(() => {
    const worker = "serviceWorker" in navigator ? navigator.serviceWorker : null;
    if (!worker) return;
    const open = (event: MessageEvent) => {
      const message = event.data as { type?: unknown; url?: unknown } | null;
      if (message?.type !== "hangar:open" || typeof message.url !== "string") return;
      const hash = hashOf(message.url, location.href);
      if (location.hash !== hash) location.hash = hash;
    };
    worker.addEventListener("message", open);
    return () => worker.removeEventListener("message", open);
  }, []);
}

/**
 * The app's frame: the window at rest, the part of the screen above the
 * on-screen keyboard while it is up (viewport.ts). Written to the root as
 * `data-keyboard`, `--keyboard-top` and `--keyboard-height`, which `.app`
 * is pinned to.
 */
export function useVisibleViewport(): void {
  useEffect(() => {
    const page = browserPage();
    return page ? watchViewport(page) : undefined;
  }, []);
}
