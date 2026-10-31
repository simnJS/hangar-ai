import { useEffect, useLayoutEffect, useRef, useState, type RefObject } from "react";
import { Icon, type IconName } from "./Icon";
import { PaneAccountItems } from "./accounts/PaneAccountItems";

/**
 * The "…" menu of a pane header: what a pane can do that is not worth a
 * button of its own in a header eight panes have to share.
 *
 * Placed in `fixed` coordinates read from its button, like the saved-commands
 * menu, so it can overhang a pane that sits against the edge of the window
 * instead of being clipped by it.
 */

/** Breathing room between the menu and its button, and the window edges. */
const GAP = 4;
const MARGIN = 8;

export interface PaneMenuItem {
  id: string;
  label: string;
  icon: IconName;
  /** The bound shortcut, already formatted; empty when nothing is bound. */
  keys?: string;
  /** Draws a hairline above the entry, to set a group apart. */
  separated?: boolean;
  onSelect: () => void;
}

interface Props {
  id: string;
  label: string;
  /** The "…" button. A press on it toggles the menu, so it does not count as
      a click outside. */
  anchorRef: RefObject<HTMLButtonElement | null>;
  items: PaneMenuItem[];
  onClose: () => void;
  /**
   * The pane this menu is for. Given, the menu ends with the Claude account
   * choice of that pane (Comptes Claude); a pane that does not run Claude
   * Code, or a menu without it, shows none.
   */
  accountFor?: { workspaceId: string; paneId: string };
}

export function PaneMenu({ id, label, anchorRef, items, onClose, accountFor }: Props) {
  const menuRef = useRef<HTMLDivElement>(null);
  const [at, setAt] = useState<{ left: number; top: number } | null>(null);

  // Measured rather than guessed: the entries depend on the pane, and the
  // height decides whether the menu fits under the button or opens upwards.
  useLayoutEffect(() => {
    const anchor = anchorRef.current;
    const menu = menuRef.current;
    if (!anchor || !menu) return;
    const button = anchor.getBoundingClientRect();
    const { width, height } = menu.getBoundingClientRect();
    const left = Math.max(
      MARGIN,
      Math.min(button.right - width, window.innerWidth - width - MARGIN),
    );
    const below = button.bottom + GAP;
    const top =
      below + height + MARGIN <= window.innerHeight
        ? below
        : Math.max(MARGIN, button.top - GAP - height);
    setAt({ left, top });
  }, [anchorRef, items.length, accountFor]);

  // The keyboard lands on the first entry, so the arrows work straight away.
  // A mouse that opened the menu sees no ring: the focus is programmatic and
  // follows a pointer press, which :focus-visible leaves alone.
  useEffect(() => {
    if (!at) return;
    menuRef.current?.querySelector<HTMLButtonElement>("[role^=menuitem]")?.focus();
  }, [at]);

  useEffect(() => {
    function onPointer(event: PointerEvent) {
      const target = event.target as Node;
      if (menuRef.current?.contains(target)) return;
      if (anchorRef.current?.contains(target)) return;
      onClose();
    }
    // Capture, so a press that lands on a terminal closes the menu before
    // xterm takes the pointer for a selection.
    window.addEventListener("pointerdown", onPointer, true);
    window.addEventListener("blur", onClose);
    return () => {
      window.removeEventListener("pointerdown", onPointer, true);
      window.removeEventListener("blur", onClose);
    };
  }, [anchorRef, onClose]);

  function onKeyDown(event: React.KeyboardEvent) {
    const entries = [
      ...(menuRef.current?.querySelectorAll<HTMLButtonElement>("[role^=menuitem]") ?? []),
    ];
    const at = entries.indexOf(document.activeElement as HTMLButtonElement);
    const go = (index: number) => entries[(index + entries.length) % entries.length]?.focus();
    switch (event.key) {
      case "ArrowDown":
        go(at + 1);
        break;
      case "ArrowUp":
        go(at < 0 ? -1 : at - 1);
        break;
      case "Home":
        go(0);
        break;
      case "End":
        go(-1);
        break;
      // Tab leaves a menu rather than walking through it, as on every desktop.
      case "Escape":
      case "Tab":
        onClose();
        break;
      default:
        return;
    }
    event.preventDefault();
    // The app's shortcuts listen on the window; a key the menu used is spent.
    event.stopPropagation();
  }

  return (
    <div
      id={id}
      ref={menuRef}
      className="pane-menu"
      role="menu"
      aria-label={label}
      onKeyDown={onKeyDown}
      style={{
        left: at?.left ?? 0,
        top: at?.top ?? 0,
        // Hidden for the one frame it takes to measure itself.
        visibility: at ? undefined : "hidden",
      }}
    >
      {items.map((item) => (
        <div key={item.id} className="pane-menu__row" role="none">
          {item.separated && <div className="pane-menu__sep" role="separator" />}
          <button
            className="pane-menu__item"
            role="menuitem"
            tabIndex={-1}
            onClick={item.onSelect}
            // The pointer and the arrows move one highlight, not two.
            onMouseEnter={(event) => event.currentTarget.focus()}
          >
            <Icon name={item.icon} size={14} />
            <span className="pane-menu__label">{item.label}</span>
            {item.keys && <kbd className="pane-menu__keys">{item.keys}</kbd>}
          </button>
        </div>
      ))}
      {accountFor && <PaneAccountItems {...accountFor} onDone={onClose} />}
    </div>
  );
}
