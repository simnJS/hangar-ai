/**
 * The app's frame on the screen. At rest it is the window itself, pinned by
 * CSS alone (`.app`, inset 0): nothing measured, so nothing can go stale.
 * Only while the on-screen keyboard is up is it pinned to the part of the
 * screen the keyboard leaves: iOS keeps the layout viewport when the keyboard
 * opens and pans over it instead, so a composer at the bottom would sit
 * under the keyboard.
 *
 * iOS reports that part through `visualViewport`, and not always well: it
 * fires `resize` as the keyboard starts to move, sometimes nothing once it
 * has, and iOS 26 has left `offsetTop` above 0 and `height` short after the
 * keyboard went away. Followed at rest, those numbers moved the whole app
 * down and cut its bottom off. So the frame is read again once the keyboard,
 * a rotation or a return to the app has settled, and dropped the moment no
 * field has the focus; the page, which never scrolls, is put back at its top
 * when iOS leaves it scrolled.
 */

/** What the page says of its viewports. */
export interface ViewportReading {
  /** The layout viewport's height: the window, whatever the keyboard. */
  layoutHeight: number;
  /** The visual viewport: the part of the window on screen. */
  height: number;
  offsetTop: number;
  scale: number;
  /** A field that takes text has the focus. */
  typing: boolean;
}

/** Where the app sits while the keyboard is up, in CSS pixels. */
export interface KeyboardFrame {
  top: number;
  height: number;
}

/**
 * Less than this taken off the window is no keyboard: a hardware keyboard's
 * shortcut bar, or what iOS 26 left behind after one (24px, 59px).
 */
export const KEYBOARD_MIN_PX = 100;

/** The frame above the keyboard, or null when there is none and the app fills the window. */
export function keyboardFrame(reading: ViewportReading): KeyboardFrame | null {
  if (!reading.typing) return null;
  // Zoomed in, the visual viewport is a magnifier, not what a keyboard left.
  if (Math.abs(reading.scale - 1) > 0.01) return null;
  if (reading.layoutHeight - reading.height < KEYBOARD_MIN_PX) return null;
  return { top: Math.max(0, Math.round(reading.offsetTop)), height: Math.round(reading.height) };
}

/** Whether a focused element takes text, so the keyboard is up for it. */
export function takesText(element: { tagName: string; isContentEditable?: boolean; type?: string } | null): boolean {
  if (!element) return false;
  if (element.isContentEditable) return true;
  const tag = element.tagName.toLowerCase();
  if (tag === "textarea") return true;
  if (tag !== "input") return false;
  return !NO_KEYBOARD.has((element.type ?? "text").toLowerCase());
}

const NO_KEYBOARD = new Set(["button", "checkbox", "color", "file", "hidden", "image", "radio", "range", "reset", "submit"]);

/**
 * When the frame is read again after something iOS animates: the keyboard
 * takes about 250ms to come or go, a rotation a little more.
 */
export const SETTLE_MS = [60, 300, 650];

/** The page, as far as its frame is concerned; the browser's is `browserPage`. */
export interface ViewportPage {
  read: () => ViewportReading;
  /** Pins the app to `frame`, or lets it fill the window again with null. */
  pin: (frame: KeyboardFrame | null) => void;
  /** How far the document is scrolled, which it should never be. */
  scrolled: () => boolean;
  scrollToTop: () => void;
  /**
   * Calls `listener` for each event that can move the frame, with whether
   * the change is one iOS animates (focus leaving, rotation, coming back).
   * Returns the unsubscribe.
   */
  subscribe: (listener: (settling: boolean) => void) => () => void;
  setTimeout: (run: () => void, ms: number) => unknown;
  clearTimeout: (timer: unknown) => void;
}

/** Keeps the app's frame on the screen; the returned function stops. */
export function watchViewport(page: ViewportPage): () => void {
  let current: KeyboardFrame | null = null;
  let timers: unknown[] = [];

  const update = () => {
    const reading = page.read();
    const next = keyboardFrame(reading);
    if (next?.top !== current?.top || next?.height !== current?.height) {
      current = next;
      page.pin(next);
    }
    // A field with the focus is iOS's to show, scrolling the page if it must.
    // Without one the document has nothing to scroll, and whatever iOS left
    // scrolled would hold the app off the screen.
    if (!reading.typing && page.scrolled()) page.scrollToTop();
  };

  const settle = () => {
    for (const timer of timers) page.clearTimeout(timer);
    timers = SETTLE_MS.map((ms) => page.setTimeout(update, ms));
  };

  const unsubscribe = page.subscribe((settling) => {
    update();
    if (settling) settle();
  });
  update();

  return () => {
    for (const timer of timers) page.clearTimeout(timer);
    timers = [];
    unsubscribe();
    if (current) page.pin(null);
    current = null;
  };
}

/** This page, in the browser: the frame is written on the root for styles.css. */
export function browserPage(): ViewportPage | null {
  const viewport = window.visualViewport;
  if (!viewport) return null;
  const root = document.documentElement;
  return {
    read: () => ({
      layoutHeight: root.clientHeight || window.innerHeight,
      height: viewport.height,
      offsetTop: viewport.offsetTop,
      scale: viewport.scale,
      typing: takesText(document.activeElement as HTMLInputElement | null),
    }),
    pin: (frame) => {
      if (frame) {
        root.style.setProperty("--keyboard-top", `${frame.top}px`);
        root.style.setProperty("--keyboard-height", `${frame.height}px`);
        root.dataset.keyboard = "";
      } else {
        delete root.dataset.keyboard;
        root.style.removeProperty("--keyboard-top");
        root.style.removeProperty("--keyboard-height");
      }
    },
    scrolled: () => window.scrollX !== 0 || window.scrollY !== 0,
    scrollToTop: () => window.scrollTo(0, 0),
    subscribe: (listener) => {
      const now = () => listener(false);
      const settling = () => listener(true);
      const visibility = () => {
        if (document.visibilityState === "visible") settling();
      };
      viewport.addEventListener("resize", now);
      viewport.addEventListener("scroll", now);
      addEventListener("scroll", now);
      addEventListener("resize", settling);
      addEventListener("orientationchange", settling);
      addEventListener("pageshow", settling);
      document.addEventListener("visibilitychange", visibility);
      document.addEventListener("focusin", settling);
      document.addEventListener("focusout", settling);
      return () => {
        viewport.removeEventListener("resize", now);
        viewport.removeEventListener("scroll", now);
        removeEventListener("scroll", now);
        removeEventListener("resize", settling);
        removeEventListener("orientationchange", settling);
        removeEventListener("pageshow", settling);
        document.removeEventListener("visibilitychange", visibility);
        document.removeEventListener("focusin", settling);
        document.removeEventListener("focusout", settling);
      };
    },
    setTimeout: (run, ms) => setTimeout(run, ms),
    clearTimeout: (timer) => clearTimeout(timer as ReturnType<typeof setTimeout>),
  };
}
