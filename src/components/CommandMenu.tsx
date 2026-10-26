import { useEffect, useLayoutEffect, useRef, useState, type RefObject } from "react";
import { useT } from "../i18n";
import { arrowStep, stepIndex } from "../lib/menuNav";
import { useShortcutLabel } from "../lib/useShortcuts";
import type { CommandId } from "../lib/shortcuts";
import type { CommandScope } from "../store";
import type { SavedCommand } from "../types";
import { Icon } from "./Icon";

/**
 * The saved commands of one pane, hanging off the ⚡ button of its header.
 *
 * Positioned in `fixed` coordinates read from the button rather than absolutely
 * inside the pane: a pane can be a quarter of the window and sit against an
 * edge, and the menu has to be able to overhang it instead of being clipped
 * down to nothing.
 */

/** Breathing room between the menu and the button, and the window edges. */
const GAP = 4;
const MARGIN = 8;

interface Props {
  /** The ⚡ button. Also the one outside click that must not close the menu:
      it toggles, and closing here first would make it reopen. */
  anchorRef: RefObject<HTMLButtonElement | null>;
  workspaceCommands: SavedCommand[];
  globalCommands: SavedCommand[];
  onRun: (command: SavedCommand) => void;
  onEdit: (command: SavedCommand, scope: CommandScope) => void;
  onRemove: (command: SavedCommand, scope: CommandScope) => void;
  onAdd: () => void;
  onClose: () => void;
}

export function CommandMenu({
  anchorRef,
  workspaceCommands,
  globalCommands,
  onRun,
  onEdit,
  onRemove,
  onAdd,
  onClose,
}: Props) {
  const menuRef = useRef<HTMLDivElement>(null);
  const [at, setAt] = useState<{ left: number; top: number } | null>(null);
  const t = useT();
  const shortcut = useShortcutLabel();

  // Measured rather than guessed: the height depends on how many commands are
  // saved, and it is what decides whether the menu fits under the button.
  useLayoutEffect(() => {
    const anchor = anchorRef.current;
    const menu = menuRef.current;
    if (!anchor || !menu) return;
    const button = anchor.getBoundingClientRect();
    const { width, height } = menu.getBoundingClientRect();

    // Right-aligned on the button, which sits at the right end of the bar.
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
  }, [anchorRef, workspaceCommands, globalCommands]);

  useEffect(() => {
    function onKey(event: KeyboardEvent) {
      const menu = menuRef.current;
      // Closing hands focus back to the pane's terminal (the header's
      // onClose), wherever it was in the menu.
      if (event.key === "Escape") {
        onClose();
        return;
      }
      // Up and Down walk the entries, from the button as well as from inside,
      // the way a menu answers the keyboard.
      const step = arrowStep(event.key);
      const active = document.activeElement;
      if (!step || !menu) return;
      if (active !== anchorRef.current && !menu.contains(active)) return;
      const items = [...menu.querySelectorAll<HTMLButtonElement>("[role='menuitem']")];
      const next = stepIndex(items.indexOf(active as HTMLButtonElement), items.length, step, true);
      if (next === null) return;
      event.preventDefault();
      items[next].focus();
    }
    function onPointer(event: PointerEvent) {
      const target = event.target as Node;
      if (menuRef.current?.contains(target)) return;
      if (anchorRef.current?.contains(target)) return;
      onClose();
    }
    // Capture, so a press that lands on a terminal closes the menu before
    // xterm takes the pointer for a selection.
    window.addEventListener("keydown", onKey);
    window.addEventListener("pointerdown", onPointer, true);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("pointerdown", onPointer, true);
    };
  }, [anchorRef, onClose]);

  const section = (commands: SavedCommand[], scope: CommandScope, offset: number) => {
    if (!commands.length) return null;
    const headId = `cmd-menu-${scope}`;
    return (
      <div className="cmd-menu__group" role="group" aria-labelledby={headId}>
        <span className="cmd-menu__head" id={headId}>
          {t(scope === "workspace" ? "commands.scopeWorkspace" : "commands.scopeGlobal")}
        </span>
        {commands.map((command, index) => {
          const rank = offset + index;
          // Only bound shortcuts are shown: these ship unbound, and promising
          // a key nobody set would be worse than saying nothing.
          const keys =
            rank < 9 ? shortcut(`command.run${rank + 1}` as CommandId) : "";
          // An entry named after its own command says it once, not twice.
          const label = command.label.trim();
          return (
            <div className="cmd-item" key={command.id}>
              <button
                className="cmd-item__run"
                role="menuitem"
                onClick={() => onRun(command)}
                title={t(command.broadcast ? "commands.runHintAll" : "commands.runHint", {
                  command: command.command,
                })}
              >
                {label ? (
                  <>
                    <span className="cmd-item__label">{label}</span>
                    <span className="cmd-item__cmd">{command.command}</span>
                  </>
                ) : (
                  <span className="cmd-item__label cmd-item__label--mono">
                    {command.command}
                  </span>
                )}
                {command.broadcast && (
                  <span className="cmd-item__tag" title={t("commands.broadcast")}>
                    ⇉
                  </span>
                )}
              </button>

              {/* The key and the edit buttons share one slot: the key at rest,
                  the buttons once the row is pointed at or focused, so neither
                  pushes the text around when the other appears. */}
              <span className="cmd-item__end">
                {keys && <span className="cmd-item__keys">{keys}</span>}
                <span className="cmd-item__actions">
                  <button
                    className="cmd-item__btn"
                    onClick={() => onEdit(command, scope)}
                    title={t("commands.edit")}
                    aria-label={t("commands.edit")}
                  >
                    <Icon name="pencil" size={12} />
                  </button>
                  <button
                    className="cmd-item__btn cmd-item__btn--del"
                    onClick={() => onRemove(command, scope)}
                    title={t("commands.remove")}
                    aria-label={t("commands.remove")}
                  >
                    <Icon name="close" size={12} />
                  </button>
                </span>
              </span>
            </div>
          );
        })}
      </div>
    );
  };

  return (
    <div
      className="cmd-menu"
      ref={menuRef}
      role="menu"
      aria-label={t("commands.menu")}
      style={{
        left: at?.left ?? 0,
        top: at?.top ?? 0,
        // Hidden for the one frame it takes to measure itself.
        visibility: at ? undefined : "hidden",
      }}
    >
      {section(workspaceCommands, "workspace", 0)}
      {section(globalCommands, "global", workspaceCommands.length)}

      <button className="cmd-menu__add" role="menuitem" onClick={onAdd}>
        <Icon name="plus" size={12} />
        {t("commands.add")}
      </button>
    </div>
  );
}
