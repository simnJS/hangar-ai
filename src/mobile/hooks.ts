import { useCallback, useEffect, useRef, useState } from "react";
import { startPolling } from "./poll";
import { parseRoute, routeHash, type Route } from "./route";

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

/**
 * The part of the screen above the on-screen keyboard: iOS keeps the layout
 * viewport when it opens and scrolls it instead, so a composer pinned to the
 * bottom would sit under the keyboard. Written to `--app-height` and
 * `--app-top` on the root, which the app's frame is pinned to.
 */
export function useVisibleViewport(): void {
  useEffect(() => {
    const viewport = window.visualViewport;
    if (!viewport) return;
    const root = document.documentElement.style;
    const update = () => {
      root.setProperty("--app-height", `${viewport.height}px`);
      root.setProperty("--app-top", `${viewport.offsetTop}px`);
    };
    update();
    viewport.addEventListener("resize", update);
    viewport.addEventListener("scroll", update);
    return () => {
      viewport.removeEventListener("resize", update);
      viewport.removeEventListener("scroll", update);
    };
  }, []);
}
