import { useEffect, useRef, useState, type RefObject } from "react";
import { getTerminal } from "./terminalRegistry";

/**
 * A side panel drawn over the terminals because there is no room for it
 * beside them (lib/sidePanels): the inspector, the board panel.
 *
 * It is a transient thing, like a menu, never a state the window is left in:
 * it takes the keyboard when it opens — it only ever opens because the user
 * asked — and goes away on Escape from anywhere, on a press anywhere else and
 * when the keyboard moves on to something else. Escape and the panel's own
 * close button give the keyboard back to whatever had it before; a press
 * elsewhere leaves it where the press put it.
 *
 * Whatever is drawn over the panel — a dialog it opened, a menu, a list — is
 * its own business: presses, focus and Escape inside those leave it alone.
 */

/** Surfaces above the panel that it must not take a press or a key from. */
const ABOVE = [
  ".modal-backdrop",
  '[aria-modal="true"]',
  '[role="dialog"]',
  '[role="alertdialog"]',
  '[role="menu"]',
  '[role="listbox"]',
].join(",");

/** The toolbar buttons that open and close a panel: their own click decides. */
export const DRAWER_TOGGLE = "data-drawer-toggle";

export type DrawerCloseReason = "escape" | "outside" | "button";

const isAbove = (target: EventTarget | null) =>
  target instanceof Element && target.closest(ABOVE) !== null;

const isToggle = (target: EventTarget | null) =>
  target instanceof Element && target.closest(`[${DRAWER_TOGGLE}]`) !== null;

export function useDrawer({
  panelRef,
  active,
  onClose,
  paneId,
}: {
  panelRef: RefObject<HTMLElement | null>;
  /** Drawn as a drawer; docked beside the terminals, none of this applies. */
  active: boolean;
  onClose: (reason: DrawerCloseReason) => void;
  /** The focused pane, which takes the keyboard back when nothing else can. */
  paneId: string | null;
}): (reason: DrawerCloseReason) => void {
  // Read during the first render, before the panel takes the focus.
  const [opener] = useState(() =>
    active && document.activeElement instanceof HTMLElement ? document.activeElement : null,
  );
  const closeRef = useRef(onClose);
  closeRef.current = onClose;
  const paneRef = useRef(paneId);
  paneRef.current = paneId;
  /** Set when a press elsewhere closed it: the press already put the focus somewhere. */
  const leftBy = useRef<DrawerCloseReason | null>(null);

  const close = (reason: DrawerCloseReason) => {
    leftBy.current = reason;
    closeRef.current(reason);
  };
  const closeLatest = useRef(close);
  closeLatest.current = close;

  useEffect(() => {
    if (!active) return;
    const panel = panelRef.current;
    if (!panel) return;
    panel.focus({ preventScroll: true });

    const outside = (target: EventTarget | null) =>
      target instanceof Node && !panel.contains(target) && !isAbove(target) && !isToggle(target);

    // Capture, so a press on a terminal closes it before xterm takes the
    // pointer for a selection.
    const onPointer = (event: PointerEvent) => {
      if (outside(event.target)) closeLatest.current("outside");
    };
    // The keyboard moving on — a shortcut focusing a pane, F6 — is leaving too.
    const onFocus = (event: FocusEvent) => {
      if (outside(event.target)) closeLatest.current("outside");
    };
    // Bubbling, so a list or a field inside that handles Escape itself keeps
    // it; a dialog open anywhere owns it.
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || event.defaultPrevented) return;
      if (isAbove(event.target) || document.querySelector(ABOVE)) return;
      event.preventDefault();
      closeLatest.current("escape");
    };

    window.addEventListener("pointerdown", onPointer, true);
    window.addEventListener("focusin", onFocus, true);
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("pointerdown", onPointer, true);
      window.removeEventListener("focusin", onFocus, true);
      window.removeEventListener("keydown", onKey);
    };
  }, [active, panelRef]);

  // Gone with the focus inside it: the keyboard goes back where it was, or to
  // the pane — not to <body>, where no key does anything. That holds docked
  // too, for a panel closed by its shortcut or squeezed into a closed drawer
  // by a narrowing window. Only a real close counts: a drawer the window grew
  // wide enough to dock keeps the focus.
  useEffect(() => {
    const panel = panelRef.current;
    if (!panel) return;
    return () => {
      if (panel.isConnected || leftBy.current === "outside") return;
      const now = document.activeElement;
      if (now && now !== document.body) return;
      if (active && opener?.isConnected) opener.focus({ preventScroll: true });
      else getTerminal(paneRef.current)?.focus();
    };
  }, [active, opener, panelRef]);

  return close;
}
