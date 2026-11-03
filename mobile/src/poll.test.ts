import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { startPolling, type Visibility } from "./poll";

/** A page whose visibility the test sets. */
function fakePage(hidden = false) {
  const listeners = new Set<() => void>();
  const visibility: Visibility = {
    hidden: () => hidden,
    subscribe: (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
  };
  const set = (value: boolean) => {
    hidden = value;
    for (const listener of listeners) listener();
  };
  return { visibility, hide: () => set(true), show: () => set(false), listeners };
}

describe("polling", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("runs at once, then every interval", async () => {
    const page = fakePage();
    const task = vi.fn(async () => {});
    const stop = startPolling(task, 2_000, page.visibility);
    await vi.advanceTimersByTimeAsync(0);
    expect(task).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(1_999);
    expect(task).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(1);
    expect(task).toHaveBeenCalledTimes(2);
    await vi.advanceTimersByTimeAsync(4_000);
    expect(task).toHaveBeenCalledTimes(4);
    stop();
  });

  it("counts the interval from the end of a slow round, never overlapping", async () => {
    const page = fakePage();
    let running = 0;
    let most = 0;
    const task = vi.fn(async () => {
      running++;
      most = Math.max(most, running);
      await new Promise((resolve) => setTimeout(resolve, 5_000));
      running--;
    });
    const stop = startPolling(task, 1_000, page.visibility);
    await vi.advanceTimersByTimeAsync(5_999);
    expect(task).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(1);
    expect(task).toHaveBeenCalledTimes(2);
    expect(most).toBe(1);
    stop();
  });

  it("asks nothing while the page is hidden, and asks at once when it is back", async () => {
    const page = fakePage();
    const task = vi.fn(async () => {});
    const stop = startPolling(task, 1_000, page.visibility);
    await vi.advanceTimersByTimeAsync(0);
    page.hide();
    await vi.advanceTimersByTimeAsync(60_000);
    expect(task).toHaveBeenCalledTimes(1);
    page.show();
    await vi.advanceTimersByTimeAsync(0);
    expect(task).toHaveBeenCalledTimes(2);
    await vi.advanceTimersByTimeAsync(1_000);
    expect(task).toHaveBeenCalledTimes(3);
    stop();
  });

  it("does not start while hidden", async () => {
    const page = fakePage(true);
    const task = vi.fn(async () => {});
    const stop = startPolling(task, 1_000, page.visibility);
    await vi.advanceTimersByTimeAsync(10_000);
    expect(task).not.toHaveBeenCalled();
    stop();
  });

  it("keeps going after a failed round", async () => {
    const page = fakePage();
    const task = vi.fn(async () => {
      throw new Error("offline");
    });
    const stop = startPolling(task, 1_000, page.visibility);
    await vi.advanceTimersByTimeAsync(3_000);
    expect(task).toHaveBeenCalledTimes(4);
    stop();
  });

  it("stops for good, and lets go of the page", async () => {
    const page = fakePage();
    const task = vi.fn(async () => {});
    const stop = startPolling(task, 1_000, page.visibility);
    await vi.advanceTimersByTimeAsync(0);
    stop();
    page.hide();
    page.show();
    await vi.advanceTimersByTimeAsync(10_000);
    expect(task).toHaveBeenCalledTimes(1);
    expect(page.listeners.size).toBe(0);
  });
});
