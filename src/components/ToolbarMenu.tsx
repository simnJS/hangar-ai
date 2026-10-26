import {
  useEffect,
  useLayoutEffect,
  useRef,
  type KeyboardEvent,
  type ReactNode,
  type RefObject,
} from "react";
import { createPortal } from "react-dom";

/**
 * A menu hanging off a toolbar button.
 *
 * Portalled to <body> and positioned in fixed coordinates read from the
 * button: the toolbar sits above the terminal grid, whose panes stack their
 * own layers, and a menu rendered in place could end up drawn under them.
 *
 * It closes on Escape, on Tab, on a press anywhere else and when the focus
 * leaves it, and says which, so the owner can decide where the focus goes
 * next — back to the button for Escape and Tab, wherever the user clicked
 * otherwise.
 *
 * Being portalled, it is the last thing in the document: Tab out of it would
 * land on nothing, or on the browser itself. So a menu closes on Tab instead
 * of letting it through, and a question keeps Tab between its own buttons.
 */

export type MenuCloseReason = "escape" | "tab" | "outside" | "blur";

/** Breathing room between the menu and its button, and the window edges. */
const GAP = 6;
const MARGIN = 8;

const ITEMS = '[role="menuitem"]:not(:disabled)';
/** What Tab walks in a question: its buttons, and a field if it has one. */
const CONTROLS = "button:not(:disabled), input:not(:disabled), [tabindex]:not([tabindex='-1'])";

export function ToolbarMenu({
  anchorRef,
  role = "menu",
  label,
  labelledBy,
  describedBy,
  onClose,
  children,
}: {
  anchorRef: RefObject<HTMLElement | null>;
  /** A menu of items, or a short question asked in its place. */
  role?: "menu" | "alertdialog";
  label?: string;
  labelledBy?: string;
  describedBy?: string;
  onClose: (reason: MenuCloseReason) => void;
  children: ReactNode;
}) {
  const ref = useRef<HTMLDivElement>(null);

  // Measured rather than guessed: the content changes size when the reset asks
  // for confirmation, and the menu stays right-aligned on its button. Written
  // straight to the node before the first paint rather than through state: a
  // menu hidden while it measures itself could not take the focus on mount,
  // and the confirm button asks for it there.
  useLayoutEffect(() => {
    const anchor = anchorRef.current;
    const menu = ref.current;
    if (!anchor || !menu) return;
    const button = anchor.getBoundingClientRect();
    const { width, height } = menu.getBoundingClientRect();
    const left = Math.max(MARGIN, Math.min(button.right - width, window.innerWidth - width - MARGIN));
    const below = button.bottom + GAP;
    const top =
      below + height + MARGIN <= window.innerHeight
        ? below
        : Math.max(MARGIN, button.top - GAP - height);
    menu.style.left = `${left}px`;
    menu.style.top = `${top}px`;
  });

  // The first item takes the focus, so the arrows work straight away. Whatever
  // asked for it — an `autoFocus` inside, a confirm button — goes first.
  useEffect(() => {
    const menu = ref.current;
    if (!menu || menu.contains(document.activeElement)) return;
    menu.querySelector<HTMLElement>(ITEMS)?.focus();
  }, [role]);

  // Latest callback without re-subscribing on every render of the owner.
  const closeRef = useRef(onClose);
  closeRef.current = onClose;

  useEffect(() => {
    function onPointer(event: PointerEvent) {
      const target = event.target as Node;
      if (ref.current?.contains(target)) return;
      // The button toggles; closing here first would make its click reopen.
      if (anchorRef.current?.contains(target)) return;
      closeRef.current("outside");
    }
    // Capture, so a press on a terminal closes the menu before xterm takes the
    // pointer for a selection.
    window.addEventListener("pointerdown", onPointer, true);
    // The menu's position was read from the button; a resize moves the button.
    const onResize = () => closeRef.current("outside");
    window.addEventListener("resize", onResize);
    return () => {
      window.removeEventListener("pointerdown", onPointer, true);
      window.removeEventListener("resize", onResize);
    };
  }, [anchorRef]);

  function onKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      closeRef.current("escape");
      return;
    }
    if (event.key === "Tab") {
      event.preventDefault();
      if (role === "menu") {
        closeRef.current("tab");
        return;
      }
      const controls = [...(ref.current?.querySelectorAll<HTMLElement>(CONTROLS) ?? [])];
      if (!controls.length) return;
      const at = controls.indexOf(document.activeElement as HTMLElement);
      const step = event.shiftKey ? -1 : 1;
      const next = at < 0 ? (event.shiftKey ? controls.length - 1 : 0) : at + step;
      controls[(next + controls.length) % controls.length].focus();
      return;
    }
    if (role !== "menu") return;
    const items = [...(ref.current?.querySelectorAll<HTMLElement>(ITEMS) ?? [])];
    if (!items.length) return;
    const at = items.indexOf(document.activeElement as HTMLElement);
    const go = (index: number) => {
      event.preventDefault();
      items[(index + items.length) % items.length].focus();
    };
    if (event.key === "ArrowDown") go(at + 1);
    else if (event.key === "ArrowUp") go(at < 0 ? -1 : at - 1);
    else if (event.key === "Home") go(0);
    else if (event.key === "End") go(-1);
  }

  return createPortal(
    <div
      ref={ref}
      className={`tmenu ${role === "alertdialog" ? "tmenu--confirm" : ""}`}
      role={role}
      aria-label={label}
      aria-labelledby={labelledBy}
      aria-describedby={describedBy}
      onKeyDown={onKeyDown}
      onBlur={(event) => {
        const next = event.relatedTarget as Node | null;
        // Null when the focus went nowhere in particular — a click on the
        // menu's own padding, the window losing focus — which is not leaving.
        if (!next || ref.current?.contains(next) || anchorRef.current?.contains(next)) return;
        closeRef.current("blur");
      }}
      // The hovered item is the current one, as in a native menu: the arrows
      // carry on from wherever the pointer left them.
      onMouseMove={(event) => {
        const item = (event.target as HTMLElement).closest<HTMLElement>(ITEMS);
        if (item && item !== document.activeElement) item.focus({ preventScroll: true });
      }}
    >
      {children}
    </div>,
    document.body,
  );
}
