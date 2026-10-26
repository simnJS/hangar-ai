import { useEffect, useId, useLayoutEffect, useRef, useState } from "react";
import { BoardIcon } from "./BoardIcons";

export interface FilterOption {
  value: string;
  label: string;
  /** What the button says when this is picked, if shorter: "All". */
  short?: string;
  /** Said after the label, quieter: the pane an agent's name leads to. */
  hint?: string;
}

interface Props {
  /** What is filtered, written before the value: "Label", "Agent". */
  name: string;
  /** Spoken name of the control, also its tooltip. */
  title: string;
  value: string;
  options: FilterOption[];
  /** A filter that hides cards is drawn in the accent, so it is never forgotten. */
  active: boolean;
  onChange: (value: string) => void;
}

/**
 * One of the board's filters: a compact button reading "Label  All ⌃" that
 * opens a list to pick from.
 *
 * A native select would do the picking, but its menu is drawn by the OS and
 * looks like nothing else in the window. This one behaves like a listbox:
 * arrows walk it, Enter picks, Escape and a click outside close it, and the
 * focus goes back to the button either way.
 */
export function BoardFilter({ name, title, value, options, active, onChange }: Props) {
  const [open, setOpen] = useState(false);
  const [cursor, setCursor] = useState(0);
  const [alignEnd, setAlignEnd] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);
  const buttonRef = useRef<HTMLButtonElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const id = useId();

  const current = options.find((option) => option.value === value) ?? options[0];

  function show() {
    setCursor(Math.max(0, options.findIndex((option) => option.value === value)));
    setAlignEnd(false);
    setOpen(true);
  }

  function close(refocus: boolean) {
    setOpen(false);
    if (refocus) buttonRef.current?.focus();
  }

  function pick(index: number) {
    const option = options[index];
    if (option) onChange(option.value);
    close(true);
  }

  // Focus moves into the list so the arrows work at once, and the list flips
  // to the button's right edge when it would run out of the window — the
  // board panel sits against it.
  useLayoutEffect(() => {
    if (!open) return;
    const list = listRef.current;
    if (!list) return;
    list.focus();
    if (list.getBoundingClientRect().right > window.innerWidth - 8) setAlignEnd(true);
  }, [open]);

  useEffect(() => {
    if (!open) return;
    const onPointer = (event: PointerEvent) => {
      if (!rootRef.current?.contains(event.target as Node)) close(false);
    };
    window.addEventListener("pointerdown", onPointer, true);
    return () => window.removeEventListener("pointerdown", onPointer, true);
  }, [open]);

  // Keeps the highlighted row in view while the arrows walk a long list.
  useEffect(() => {
    if (!open) return;
    listRef.current
      ?.querySelector<HTMLElement>(`[data-index="${cursor}"]`)
      ?.scrollIntoView({ block: "nearest" });
  }, [open, cursor]);

  function onListKey(event: React.KeyboardEvent) {
    const last = options.length - 1;
    switch (event.key) {
      case "ArrowDown":
        setCursor((at) => Math.min(last, at + 1));
        break;
      case "ArrowUp":
        setCursor((at) => Math.max(0, at - 1));
        break;
      case "Home":
        setCursor(0);
        break;
      case "End":
        setCursor(last);
        break;
      case "Enter":
      case " ":
        pick(cursor);
        break;
      case "Escape":
        close(true);
        break;
      case "Tab":
        close(false);
        return;
      default:
        return;
    }
    // Escape stops here: it closes this list, not the dialog or view under it.
    event.preventDefault();
    event.stopPropagation();
  }

  return (
    <div className="board-filter" ref={rootRef}>
      <button
        ref={buttonRef}
        type="button"
        className={`board-filter__button ${active ? "is-active" : ""}`}
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-controls={open ? `${id}-list` : undefined}
        aria-label={`${title}: ${current?.label ?? ""}`}
        title={title}
        onClick={() => (open ? close(true) : show())}
        onKeyDown={(event) => {
          if (event.key === "ArrowDown" || event.key === "ArrowUp") {
            event.preventDefault();
            show();
          }
        }}
      >
        <span className="board-filter__name">{name}</span>
        <span className="board-filter__value">{current?.short ?? current?.label}</span>
        <BoardIcon name="updown" size={10} />
      </button>

      {open && (
        <div
          ref={listRef}
          id={`${id}-list`}
          className={`board-filter__menu ${alignEnd ? "board-filter__menu--end" : ""}`}
          role="listbox"
          aria-label={title}
          aria-activedescendant={`${id}-${cursor}`}
          tabIndex={-1}
          onKeyDown={onListKey}
        >
          {options.map((option, index) => {
            const selected = option.value === value;
            return (
              <div
                key={option.value}
                id={`${id}-${index}`}
                data-index={index}
                role="option"
                aria-selected={selected}
                className={`board-filter__option ${index === cursor ? "is-cursor" : ""}`}
                onMouseMove={() => setCursor(index)}
                onClick={() => pick(index)}
              >
                <span className="board-filter__check" aria-hidden>
                  {selected && <BoardIcon name="check" size={11} />}
                </span>
                <span className="board-filter__label">{option.label}</span>
                {option.hint && <span className="board-filter__hint">{option.hint}</span>}
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
