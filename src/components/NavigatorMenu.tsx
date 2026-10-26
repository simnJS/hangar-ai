import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { NavigatorIcon, type NavigatorIconName } from "./NavigatorIcons";

/** Kept between the menu and the window edges. */
const MARGIN = 8;

export type NavigatorMenuEntry =
  | {
      kind: "item";
      label: string;
      icon?: NavigatorIconName;
      onSelect: () => void;
      /** Destroys something: drawn in the danger colour. */
      danger?: boolean;
      /** A choice among several, ticked when it is the current one. */
      checked?: boolean;
      disabled?: boolean;
      title?: string;
      /** Puts the focus somewhere of its own (a field, a panel): the row does not take it back. */
      takesFocus?: boolean;
    }
  | { kind: "heading"; label: string }
  | { kind: "separator" };

interface Props {
  /** Where it opens, in window coordinates: the pointer, or under the row. */
  x: number;
  y: number;
  label: string;
  entries: NavigatorMenuEntry[];
  /** Opened from the keyboard: the first item takes the focus straight away. */
  fromKeyboard: boolean;
  onClose: () => void;
  /**
   * Gives the focus back to the row the menu is about. Asked of the owner
   * rather than done here: what was picked may have moved or removed it.
   */
  onRestoreFocus: () => void;
}

const ITEMS = '[role^="menuitem"]:not(:disabled)';

/**
 * The navigator's context menu: right-click, the "…" of a row, or the menu
 * key on a focused row. Arrow keys walk it, Escape closes it and gives the
 * focus back to its row; a click elsewhere just closes it.
 *
 * The pointer moves the focus with it, so the keyboard and the mouse never
 * highlight two different items at once.
 */
export function NavigatorMenu({
  x,
  y,
  label,
  entries,
  fromKeyboard,
  onClose,
  onRestoreFocus,
}: Props) {
  const menuRef = useRef<HTMLDivElement>(null);
  const [at, setAt] = useState<{ left: number; top: number } | null>(null);

  // Opens at the pointer, pulled back inside the window when a click near an
  // edge would leave it hanging off it.
  useLayoutEffect(() => {
    const menu = menuRef.current;
    if (!menu) return;
    const { width, height } = menu.getBoundingClientRect();
    setAt({
      left: Math.max(MARGIN, Math.min(x, window.innerWidth - width - MARGIN)),
      top: Math.max(MARGIN, Math.min(y, window.innerHeight - height - MARGIN)),
    });
  }, [x, y]);

  // Once placed: a hidden element cannot take the focus, and the menu is
  // hidden for the frame it takes to measure itself.
  const placed = at !== null;
  useEffect(() => {
    const menu = menuRef.current;
    if (!placed || !menu) return;
    const first = menu.querySelector<HTMLElement>(ITEMS);
    (fromKeyboard && first ? first : menu).focus({ preventScroll: true });
  }, [placed, fromKeyboard]);

  useEffect(() => {
    function onPointer(event: PointerEvent) {
      if (!menuRef.current?.contains(event.target as Node)) onClose();
    }
    // Capture, so a press on a terminal closes the menu before xterm takes the
    // pointer for a selection.
    window.addEventListener("pointerdown", onPointer, true);
    window.addEventListener("blur", onClose);
    return () => {
      window.removeEventListener("pointerdown", onPointer, true);
      window.removeEventListener("blur", onClose);
    };
  }, [onClose]);

  function closeBack() {
    onClose();
    onRestoreFocus();
  }

  function onKeyDown(event: React.KeyboardEvent<HTMLDivElement>) {
    const items = [...(menuRef.current?.querySelectorAll<HTMLElement>(ITEMS) ?? [])];
    const at = items.indexOf(document.activeElement as HTMLElement);
    const go = (index: number) => {
      event.preventDefault();
      items[(index + items.length) % items.length]?.focus();
    };
    switch (event.key) {
      case "ArrowDown":
        return go(at + 1);
      case "ArrowUp":
        return go(at < 0 ? -1 : at - 1);
      case "Home":
        return go(0);
      case "End":
        return go(-1);
      case "Escape":
        event.preventDefault();
        event.stopPropagation();
        return closeBack();
      case "Tab":
        event.preventDefault();
        return closeBack();
    }
  }

  return (
    <div
      className="ctx-menu"
      ref={menuRef}
      role="menu"
      aria-label={label}
      tabIndex={-1}
      onKeyDown={onKeyDown}
      // Leaving the menu leaves nothing highlighted, as a native menu does.
      onPointerLeave={() => menuRef.current?.focus({ preventScroll: true })}
      onContextMenu={(event) => event.preventDefault()}
      style={{
        left: at?.left ?? 0,
        top: at?.top ?? 0,
        // Hidden for the one frame it takes to measure itself.
        visibility: at ? undefined : "hidden",
      }}
    >
      {entries.map((entry, index) => {
        if (entry.kind === "separator") {
          return <div key={index} className="ctx-menu__sep" role="separator" />;
        }
        if (entry.kind === "heading") {
          return (
            <div key={index} className="ctx-menu__head" role="presentation">
              {entry.label}
            </div>
          );
        }
        const radio = entry.checked !== undefined;
        return (
          <button
            key={index}
            className={`ctx-menu__item ${entry.danger ? "ctx-menu__item--danger" : ""}`}
            role={radio ? "menuitemradio" : "menuitem"}
            aria-checked={radio ? entry.checked : undefined}
            disabled={entry.disabled}
            title={entry.title}
            tabIndex={-1}
            onPointerMove={(event) => {
              if (document.activeElement !== event.currentTarget) {
                event.currentTarget.focus({ preventScroll: true });
              }
            }}
            onClick={() => {
              entry.onSelect();
              if (entry.takesFocus) onClose();
              else closeBack();
            }}
          >
            <span className="ctx-menu__icon">
              {entry.icon && <NavigatorIcon name={entry.icon} size={14} />}
            </span>
            <span className="ctx-menu__label">{entry.label}</span>
            {entry.checked && (
              <span className="ctx-menu__check">
                <NavigatorIcon name="check" size={13} />
              </span>
            )}
          </button>
        );
      })}
    </div>
  );
}
