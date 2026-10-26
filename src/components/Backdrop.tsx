import { useEffect, useRef, useState, type KeyboardEvent, type ReactNode } from "react";
import { useT } from "../i18n";
import { focusablesIn } from "../lib/focusable";
import { Icon } from "./Icon";

/**
 * Keeps Tab inside a modal surface and gives focus back when it goes away.
 *
 * The element focused before the surface opened is read during the first
 * render, before anything inside it can take focus, and is restored only on a
 * real unmount — when the surface is gone from the document — and only if
 * nothing else claimed focus meanwhile: a dialog whose action focuses a
 * terminal must not have that undone by its own cleanup.
 */
export function useModalFocus() {
  const rootRef = useRef<HTMLDivElement>(null);
  const [opener] = useState(() =>
    document.activeElement instanceof HTMLElement ? document.activeElement : null,
  );

  useEffect(() => {
    const root = rootRef.current;
    if (!root) return;
    // A field with autoFocus already holds it; otherwise the first control of
    // the body rather than the close button, which is what a keyboard user
    // reaches for last.
    if (!root.contains(document.activeElement)) {
      const body = root.querySelector<HTMLElement>("[data-modal-body], .modal__body");
      const target = (body && focusablesIn(body)[0]) ?? focusablesIn(root)[0] ?? root;
      target.focus({ preventScroll: true });
    }
    return () => {
      if (root.isConnected) return;
      const active = document.activeElement;
      if (active && active !== document.body) return;
      if (opener?.isConnected) opener.focus({ preventScroll: true });
    };
  }, [opener]);

  /** Wraps Tab and Shift+Tab around the ends of the surface. */
  const onKeyDown = (event: KeyboardEvent) => {
    // A nested surface has already wrapped it.
    if (event.key !== "Tab" || event.defaultPrevented) return;
    const root = rootRef.current;
    if (!root) return;
    const items = focusablesIn(root);
    if (!items.length) {
      event.preventDefault();
      return;
    }
    const first = items[0];
    const last = items[items.length - 1];
    const active = document.activeElement;
    const outside = !active || active === root || !root.contains(active);
    if (event.shiftKey && (active === first || outside)) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && (active === last || outside)) {
      event.preventDefault();
      first.focus();
    }
  };

  return { rootRef, onKeyDown };
}

/**
 * Dismisses only when the press *and* the release both land on the backdrop.
 *
 * A plain onClick would fire whenever mousedown and mouseup share an ancestor:
 * selecting text inside the dialog and releasing outside it would close the
 * dialog mid-edit.
 */
export function Backdrop({
  onClose,
  children,
  className = "",
  escape = true,
}: {
  onClose: () => void;
  children: ReactNode;
  className?: string;
  /** Off for a surface that steers Escape itself (the settings clear their
      search before they close). */
  escape?: boolean;
}) {
  const pressedBackdrop = useRef(false);
  const { rootRef, onKeyDown } = useModalFocus();

  useEffect(() => {
    if (!escape) return;
    function onKey(event: globalThis.KeyboardEvent) {
      if (event.key === "Escape") onClose();
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose, escape]);

  return (
    <div
      ref={rootRef}
      className={`modal-backdrop ${className}`}
      // Focusable from script only: a click on the scrim parks focus here,
      // still inside the trap, rather than on the page underneath.
      tabIndex={-1}
      onKeyDown={onKeyDown}
      onMouseDown={(event) => {
        pressedBackdrop.current = event.target === event.currentTarget;
      }}
      onMouseUp={(event) => {
        const shouldClose = pressedBackdrop.current && event.target === event.currentTarget;
        pressedBackdrop.current = false;
        if (shouldClose) onClose();
      }}
    >
      {children}
    </div>
  );
}

/**
 * The title bar every dialog shares: a title, an optional monospace subtitle
 * (the folder it acts on), and the close button.
 */
export function ModalHead({
  title,
  sub,
  onClose,
  id,
}: {
  title: ReactNode;
  sub?: string;
  onClose: () => void;
  /** Lets the dialog name itself after its title (`aria-labelledby`). */
  id?: string;
}) {
  const t = useT();
  return (
    <header className="modal__head">
      <div className="modal__titles">
        <h2 className="modal__title" id={id}>
          {title}
        </h2>
        {sub && (
          <p className="modal__sub" title={sub}>
            {sub}
          </p>
        )}
      </div>
      <button
        type="button"
        className="modal__close"
        onClick={onClose}
        aria-label={t("modal.close")}
        title={t("keymap.withKeys", { label: t("modal.close"), keys: "Esc" })}
      >
        <Icon name="close" />
      </button>
    </header>
  );
}
