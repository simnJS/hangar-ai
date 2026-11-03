/**
 * Polling that only runs while the page is seen: a phone in a pocket asks
 * the PC nothing. Each round starts `intervalMs` after the previous one has
 * settled, so a slow answer never piles calls up; the page coming back runs
 * one at once.
 */

export interface Visibility {
  hidden: () => boolean;
  /** Calls `listener` when the page is hidden or shown; returns the unsubscribe. */
  subscribe: (listener: () => void) => () => void;
}

export const pageVisibility: Visibility = {
  hidden: () => document.visibilityState === "hidden",
  subscribe: (listener) => {
    document.addEventListener("visibilitychange", listener);
    return () => document.removeEventListener("visibilitychange", listener);
  },
};

/** Starts polling `task`; the returned function stops it. */
export function startPolling(
  task: () => Promise<unknown>,
  intervalMs: number,
  visibility: Visibility = pageVisibility,
): () => void {
  let stopped = false;
  let running = false;
  let timer: ReturnType<typeof setTimeout> | null = null;

  const clear = () => {
    if (timer !== null) clearTimeout(timer);
    timer = null;
  };

  const run = async () => {
    clear();
    if (stopped || running || visibility.hidden()) return;
    running = true;
    try {
      await task();
    } catch {
      // The task reports its own failures; polling carries on.
    } finally {
      running = false;
    }
    if (!stopped && !visibility.hidden()) timer = setTimeout(run, intervalMs);
  };

  const unsubscribe = visibility.subscribe(() => {
    if (visibility.hidden()) clear();
    else void run();
  });
  void run();

  return () => {
    stopped = true;
    clear();
    unsubscribe();
  };
}
