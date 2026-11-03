import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { KEYBOARD_MIN_PX, SETTLE_MS, keyboardFrame, takesText, watchViewport, type KeyboardFrame, type ViewportPage, type ViewportReading } from "./viewport";

/** An iPhone 15 Pro's window, at rest. */
const REST: ViewportReading = { layoutHeight: 852, height: 852, offsetTop: 0, scale: 1, typing: false };

/** The same with the keyboard up and the composer focused: iOS has panned to it. */
const KEYBOARD: ViewportReading = { layoutHeight: 852, height: 516, offsetTop: 336, scale: 1, typing: true };

describe("the frame above the keyboard", () => {
  it("is none at rest: the app fills the window", () => {
    expect(keyboardFrame(REST)).toBeNull();
  });

  it("is the visual viewport while the keyboard is up", () => {
    expect(keyboardFrame(KEYBOARD)).toEqual({ top: 336, height: 516 });
  });

  it("is none once no field has the focus, whatever iOS still reports", () => {
    // iOS 26 left offsetTop above 0 and the height short after the keyboard.
    expect(keyboardFrame({ ...KEYBOARD, typing: false })).toBeNull();
    expect(keyboardFrame({ ...REST, height: 828, offsetTop: 59 })).toBeNull();
  });

  it("is none for a field focused without an on-screen keyboard", () => {
    expect(keyboardFrame({ ...REST, typing: true })).toBeNull();
    // A hardware keyboard's bar, or what iOS 26 left of a keyboard gone.
    expect(keyboardFrame({ ...REST, typing: true, height: 852 - 59, offsetTop: 59 })).toBeNull();
    expect(keyboardFrame({ ...REST, typing: true, height: 852 - (KEYBOARD_MIN_PX - 1) })).toBeNull();
    expect(keyboardFrame({ ...REST, typing: true, height: 852 - KEYBOARD_MIN_PX })).toEqual({ top: 0, height: 752 });
  });

  it("is none while zoomed in, the visual viewport a magnifier", () => {
    expect(keyboardFrame({ ...KEYBOARD, scale: 1.6 })).toBeNull();
  });

  it("rounds to whole pixels and never goes above the window", () => {
    expect(keyboardFrame({ ...KEYBOARD, height: 515.6, offsetTop: 336.4 })).toEqual({ top: 336, height: 516 });
    expect(keyboardFrame({ ...KEYBOARD, offsetTop: -12 })).toEqual({ top: 0, height: 516 });
  });

  it("follows the window when it rotates", () => {
    const landscape = { layoutHeight: 393, height: 393 - 200, offsetTop: 200, scale: 1, typing: true };
    expect(keyboardFrame(landscape)).toEqual({ top: 200, height: 193 });
  });
});

describe("a field that takes text", () => {
  it("is a textarea, a text input or an editable element", () => {
    expect(takesText({ tagName: "TEXTAREA" })).toBe(true);
    expect(takesText({ tagName: "INPUT", type: "text" })).toBe(true);
    expect(takesText({ tagName: "INPUT", type: "search" })).toBe(true);
    expect(takesText({ tagName: "INPUT" })).toBe(true);
    expect(takesText({ tagName: "DIV", isContentEditable: true })).toBe(true);
  });

  it("is not a button, a choice or the page", () => {
    expect(takesText({ tagName: "BUTTON" })).toBe(false);
    expect(takesText({ tagName: "INPUT", type: "radio" })).toBe(false);
    expect(takesText({ tagName: "INPUT", type: "checkbox" })).toBe(false);
    expect(takesText({ tagName: "INPUT", type: "file" })).toBe(false);
    expect(takesText({ tagName: "BODY" })).toBe(false);
    expect(takesText(null)).toBe(false);
  });
});

/** A page whose viewports the test moves as iOS would. */
function fakePage(reading: ViewportReading = REST) {
  const listeners = new Set<(settling: boolean) => void>();
  const pins: (KeyboardFrame | null)[] = [];
  let scrollY = 0;
  const page: ViewportPage = {
    read: () => reading,
    pin: (frame) => pins.push(frame),
    scrolled: () => scrollY !== 0,
    scrollToTop: () => {
      scrollY = 0;
    },
    subscribe: (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    setTimeout: (run, ms) => setTimeout(run, ms),
    clearTimeout: (timer) => clearTimeout(timer as ReturnType<typeof setTimeout>),
  };
  const fire = (settling: boolean) => {
    for (const listener of listeners) listener(settling);
  };
  return {
    page,
    pins,
    listeners,
    /** The frame the app is pinned to now: null when it fills the window. */
    frame: () => (pins.length ? pins[pins.length - 1] : null),
    scrollY: () => scrollY,
    /** iOS changes its numbers; no event says so yet. */
    set: (next: Partial<ViewportReading>) => {
      reading = { ...reading, ...next };
    },
    /** A visualViewport resize or scroll. */
    viewportEvent: () => fire(false),
    /** Focus in or out, a rotation, the app shown again. */
    settlingEvent: () => fire(true),
    scrollBy: (y: number) => {
      scrollY += y;
    },
  };
}

describe("keeping the app's frame on the screen", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("pins nothing at rest, so the app fills the window from CSS alone", () => {
    const page = fakePage();
    const stop = watchViewport(page.page);
    page.viewportEvent();
    page.settlingEvent();
    vi.runAllTimers();
    expect(page.pins).toEqual([]);
    stop();
  });

  it("pins the app above the keyboard, follows iOS panning, and lets go when it closes", () => {
    const page = fakePage();
    const stop = watchViewport(page.page);
    page.set({ typing: true });
    page.settlingEvent();
    page.set(KEYBOARD);
    page.viewportEvent();
    expect(page.frame()).toEqual({ top: 336, height: 516 });
    page.set({ offsetTop: 300 });
    page.viewportEvent();
    expect(page.frame()).toEqual({ top: 300, height: 516 });
    page.set(REST);
    page.settlingEvent();
    expect(page.frame()).toBeNull();
    stop();
  });

  it("drops the frame on blur even when iOS leaves the visual viewport stale", () => {
    const page = fakePage(KEYBOARD);
    const stop = watchViewport(page.page);
    expect(page.frame()).toEqual({ top: 336, height: 516 });
    // The keyboard is gone; iOS 26 still says it is panned, and fires nothing more.
    page.set({ typing: false });
    page.settlingEvent();
    expect(page.frame()).toBeNull();
    vi.runAllTimers();
    expect(page.frame()).toBeNull();
    stop();
  });

  it("reads the keyboard's height again once it has finished coming up", () => {
    const page = fakePage();
    const stop = watchViewport(page.page);
    page.set({ typing: true });
    page.settlingEvent();
    // iOS fires resize as the keyboard starts to move, then nothing.
    page.set({ height: 760, offsetTop: 92 });
    page.viewportEvent();
    expect(page.frame()).toBeNull();
    page.set(KEYBOARD);
    vi.advanceTimersByTime(SETTLE_MS[SETTLE_MS.length - 1]);
    expect(page.frame()).toEqual({ top: 336, height: 516 });
    stop();
  });

  it("lets go of a keyboard that closes with no event once it has settled", () => {
    const page = fakePage(KEYBOARD);
    const stop = watchViewport(page.page);
    // At the blur the field still reads as focused, the keyboard as up.
    page.settlingEvent();
    expect(page.frame()).toEqual({ top: 336, height: 516 });
    // Then focus and keyboard go, and iOS fires nothing more.
    page.set(REST);
    vi.runAllTimers();
    expect(page.frame()).toBeNull();
    stop();
  });

  it("puts back at its top a page iOS left scrolled after the keyboard", () => {
    const page = fakePage(KEYBOARD);
    const stop = watchViewport(page.page);
    page.scrollBy(336);
    page.viewportEvent();
    // While the field has the focus, the scroll is iOS's to show it.
    expect(page.scrollY()).toBe(336);
    page.set({ ...REST });
    page.settlingEvent();
    expect(page.scrollY()).toBe(0);
    expect(page.frame()).toBeNull();
    stop();
  });

  it("puts the page back as soon as it scrolls at rest", () => {
    const page = fakePage();
    const stop = watchViewport(page.page);
    page.scrollBy(59);
    page.viewportEvent();
    expect(page.scrollY()).toBe(0);
    stop();
  });

  it("comes back from another app as it left, the keyboard gone though the field kept the focus", () => {
    const page = fakePage(KEYBOARD);
    const stop = watchViewport(page.page);
    // Away, the keyboard went; back, the field has the focus and no keyboard yet.
    page.set({ height: 852, offsetTop: 0 });
    page.settlingEvent();
    expect(page.frame()).toBeNull();
    stop();
  });

  it("measures again after a rotation, once it has settled", () => {
    const page = fakePage();
    const stop = watchViewport(page.page);
    page.set({ typing: true });
    page.settlingEvent();
    page.set({ layoutHeight: 393, height: 393 - 210, offsetTop: 210 });
    vi.runAllTimers();
    expect(page.frame()).toEqual({ top: 210, height: 183 });
    stop();
  });

  it("writes the frame only when it changes", () => {
    const page = fakePage(KEYBOARD);
    const stop = watchViewport(page.page);
    page.viewportEvent();
    page.settlingEvent();
    vi.runAllTimers();
    expect(page.pins).toEqual([{ top: 336, height: 516 }]);
    stop();
  });

  it("stops for good: no listener, no timer, the app left to fill the window", () => {
    const page = fakePage(KEYBOARD);
    const stop = watchViewport(page.page);
    page.settlingEvent();
    stop();
    expect(page.listeners.size).toBe(0);
    expect(vi.getTimerCount()).toBe(0);
    expect(page.frame()).toBeNull();
  });
});
