import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createActivityWatcher, type ActivityWatcher } from "./activity";

const IDLE_MS = 1000;

/**
 * The watcher only needs `window.setInterval`, `window.clearInterval` and
 * `Date.now`: under Node, `window` is the global object, whose timers and clock
 * are the fake ones.
 */
beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(new Date("2026-01-01T00:00:00Z"));
  vi.stubGlobal("window", globalThis);
});

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

function watch(idleMs: () => number = () => IDLE_MS) {
  const onSettle = vi.fn();
  const onBusy = vi.fn();
  const watcher = createActivityWatcher({ idleMs, onSettle, onBusy });
  return { watcher, onSettle, onBusy };
}

/** Output every `everyMs` for `durationMs`, ending on a last chunk. */
function output(watcher: ActivityWatcher, durationMs: number, everyMs = 100) {
  for (let elapsed = 0; elapsed < durationMs; elapsed += everyMs) {
    watcher.push();
    vi.advanceTimersByTime(everyMs);
  }
  watcher.push();
}

describe("createActivityWatcher", () => {
  it("settles once a long burst falls silent", () => {
    const { watcher, onSettle } = watch();
    output(watcher, 4000);

    vi.advanceTimersByTime(IDLE_MS - 1);
    expect(onSettle).not.toHaveBeenCalled();

    // The silence is checked on a 250 ms tick, so it lands within one of them.
    vi.advanceTimersByTime(250);
    expect(onSettle).toHaveBeenCalledTimes(1);

    vi.advanceTimersByTime(10_000);
    expect(onSettle).toHaveBeenCalledTimes(1);
  });

  it("ignores a short burst, the way a prompt or an echoed key looks", () => {
    const { watcher, onSettle } = watch();
    output(watcher, 1000);
    vi.advanceTimersByTime(10_000);
    expect(onSettle).not.toHaveBeenCalled();
    // And stops ticking once the burst is over.
    expect(vi.getTimerCount()).toBe(0);
  });

  it("does not count the silence itself as work", () => {
    const { watcher, onSettle } = watch(() => 5000);
    // 2 s of output then 5 s of silence: 7 s since the start, 2 s of work.
    output(watcher, 2000);
    vi.advanceTimersByTime(6000);
    expect(onSettle).not.toHaveBeenCalled();
  });

  it("keeps waiting while output keeps coming", () => {
    const { watcher, onSettle } = watch();
    output(watcher, 20_000, 900);
    expect(onSettle).not.toHaveBeenCalled();
    vi.advanceTimersByTime(IDLE_MS + 250);
    expect(onSettle).toHaveBeenCalledTimes(1);
  });

  it("reads the idle delay on every tick", () => {
    let idle = 60_000;
    const { watcher, onSettle } = watch(() => idle);
    output(watcher, 4000);
    vi.advanceTimersByTime(3000);
    expect(onSettle).not.toHaveBeenCalled();

    idle = 1000;
    vi.advanceTimersByTime(250);
    expect(onSettle).toHaveBeenCalledTimes(1);
  });

  it("settles on a bell without waiting for the silence", () => {
    const { watcher, onSettle } = watch();
    output(watcher, 500);
    watcher.ring();
    expect(onSettle).toHaveBeenCalledTimes(1);
    expect(vi.getTimerCount()).toBe(0);
  });

  it("counts the silence and a bell right after it as one hand-back", () => {
    const { watcher, onSettle } = watch();
    output(watcher, 4000);
    vi.advanceTimersByTime(IDLE_MS + 250);
    expect(onSettle).toHaveBeenCalledTimes(1);
    // The agent rings as its prompt comes back, just after going quiet.
    watcher.ring();
    expect(onSettle).toHaveBeenCalledTimes(1);
  });

  it("lets two bells through once they are far enough apart", () => {
    const { watcher, onSettle } = watch();
    watcher.ring();
    vi.advanceTimersByTime(1000);
    watcher.ring();
    expect(onSettle).toHaveBeenCalledTimes(1);
    vi.advanceTimersByTime(600);
    watcher.ring();
    expect(onSettle).toHaveBeenCalledTimes(2);
  });

  it("stops for good once disposed", () => {
    const { watcher, onSettle } = watch();
    output(watcher, 4000);
    watcher.dispose();
    vi.advanceTimersByTime(10_000);
    expect(onSettle).not.toHaveBeenCalled();
    expect(vi.getTimerCount()).toBe(0);
  });

  describe("onBusy", () => {
    it("fires once a burst is long enough to be work, and only once", () => {
      const { watcher, onBusy } = watch();
      output(watcher, 2000);
      expect(onBusy).not.toHaveBeenCalled();

      output(watcher, 6000);
      expect(onBusy).toHaveBeenCalledTimes(1);
    });

    it("never fires for a short burst", () => {
      const { watcher, onBusy } = watch();
      output(watcher, 1500);
      vi.advanceTimersByTime(10_000);
      expect(onBusy).not.toHaveBeenCalled();
    });

    it("fires again for the next burst once the last one settled", () => {
      const { watcher, onBusy, onSettle } = watch();
      output(watcher, 4000);
      vi.advanceTimersByTime(IDLE_MS + 250);
      expect(onSettle).toHaveBeenCalledTimes(1);

      output(watcher, 4000);
      expect(onBusy).toHaveBeenCalledTimes(2);
    });
  });
});
