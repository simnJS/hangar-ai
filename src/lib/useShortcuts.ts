import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  chordFromEvent,
  isBareChord,
  isTextEditingChord,
  serializeChords,
  type Chord,
} from "./keys";
import {
  indexKeymap,
  resolveKeymap,
  shortcutLabel,
  type CommandId,
  type Keymap,
} from "./shortcuts";
import { useStore } from "../store";
import { useT } from "../i18n";

/**
 * The one keyboard listener of the app.
 *
 * It sits on the capture phase of `window`, above everything else: left to
 * propagate, Ctrl+Shift+Enter would also reach the broadcast input — which
 * sends on Enter — and the focused terminal, so one keystroke would do two
 * things. Consuming here rather than filtering on the target is also what keeps
 * shortcuts working from inside a terminal, whose xterm surface is a
 * <textarea>.
 */

export type ShortcutHandlers = Partial<Record<CommandId, () => void>>;

/** How long a half-typed sequence waits for the chord that would complete it. */
const SEQUENCE_TIMEOUT_MS = 1600;

/**
 * Recording a shortcut means typing keys that are themselves shortcuts, so the
 * dispatcher steps aside while a recorder is open. A counter rather than a
 * boolean: two recorders closing in any order still leave it balanced.
 */
let suspensions = 0;

export function suspendShortcuts(): () => void {
  suspensions++;
  let released = false;
  return () => {
    if (released) return;
    released = true;
    suspensions--;
  };
}

/**
 * True while a recorder owns the keyboard. Anything else listening on the
 * window has to stand down too — Escape cancels the recording, and must not
 * also close the page the recorder is sitting in.
 */
export const shortcutsSuspended = () => suspensions > 0;

/** Text the user is typing into, xterm's hidden textarea excepted. */
function isTypingTarget(target: EventTarget | null): boolean {
  const el = target as HTMLElement | null;
  if (!el || typeof el.closest !== "function") return false;
  if (el.closest(".xterm")) return false;
  if (el.isContentEditable) return true;
  const tag = el.tagName;
  return tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT";
}

function consume(event: KeyboardEvent) {
  event.preventDefault();
  event.stopPropagation();
}

/**
 * Installs the dispatcher and reports the chords of a sequence still waiting
 * for its next key, so the UI can show what it is holding.
 */
export function useShortcuts(options: {
  keymap: Keymap;
  handlers: ShortcutHandlers;
  enabled?: boolean;
}): Chord[] {
  const { keymap, handlers, enabled = true } = options;

  const index = useMemo(() => indexKeymap(keymap), [keymap]);
  const [pending, setPending] = useState<Chord[]>([]);

  // The listener is installed once; everything it reads changes under it.
  const indexRef = useRef(index);
  indexRef.current = index;
  const handlersRef = useRef(handlers);
  handlersRef.current = handlers;
  const enabledRef = useRef(enabled);
  enabledRef.current = enabled;

  const pendingRef = useRef<Chord[]>([]);
  const timerRef = useRef<number | null>(null);

  useEffect(() => {
    function forget() {
      if (timerRef.current) window.clearTimeout(timerRef.current);
      timerRef.current = null;
      if (pendingRef.current.length) {
        pendingRef.current = [];
        setPending([]);
      }
    }

    function hold(chords: Chord[]) {
      if (timerRef.current) window.clearTimeout(timerRef.current);
      pendingRef.current = chords;
      setPending(chords);
      timerRef.current = window.setTimeout(forget, SEQUENCE_TIMEOUT_MS);
    }

    function onKey(event: KeyboardEvent) {
      if (!enabledRef.current || suspensions > 0) {
        forget();
        return;
      }

      const chord = chordFromEvent(event);
      // A modifier pressed on its own is not a keystroke yet — and must not
      // cancel a sequence half way through.
      if (!chord) return;

      const armed = pendingRef.current.length > 0;

      // A shortcut without Ctrl/Alt/Meta is a character somewhere else; typing
      // it into a field is not asking for the command. Mid-sequence the same
      // key is unambiguous, so the guard only applies to a first chord.
      if (!armed && isBareChord(chord) && isTypingTarget(event.target)) return;

      // Copy, paste and select-all belong to the field the caret is in. On a
      // Mac they are also the terminal's own bindings, which is why this is not
      // covered by the check above: Cmd+C is anything but a bare chord.
      if (!armed && isTextEditingChord(chord) && isTypingTarget(event.target)) return;

      const chords = [...pendingRef.current, chord];
      const serial = serializeChords(chords);
      const owners = indexRef.current.exact.get(serial) ?? [];

      // Several commands can share a keystroke — the settings flag it as a
      // conflict — and the first one that has something to do here wins.
      const handler = owners
        .map((id) => handlersRef.current[id])
        .find((fn): fn is () => void => Boolean(fn));

      if (handler) {
        consume(event);
        forget();
        handler();
        return;
      }

      if (indexRef.current.prefixes.has(serial)) {
        consume(event);
        hold(chords);
        return;
      }

      // The sequence led nowhere: swallow the key that ended it rather than
      // letting half a shortcut land in the terminal.
      if (armed) {
        consume(event);
        forget();
      }
    }

    window.addEventListener("keydown", onKey, true);
    return () => {
      window.removeEventListener("keydown", onKey, true);
      if (timerRef.current) window.clearTimeout(timerRef.current);
    };
  }, []);

  return pending;
}

/** Every command's bindings, the user's overrides merged over the defaults. */
export function useKeymap(): Keymap {
  const { state } = useStore();
  const overrides = state.settings.keybindings;
  return useMemo(() => resolveKeymap(overrides), [overrides]);
}

/** Formatted first binding of a command; empty when nothing is bound to it. */
export function useShortcutLabel(): (id: CommandId) => string {
  const keymap = useKeymap();
  return useCallback((id: CommandId) => shortcutLabel(keymap, id), [keymap]);
}

/**
 * `"Close pane (Ctrl+Shift+X)"` — a label followed by the key that runs it, so
 * a tooltip stays truthful after the user re-binds the command.
 */
export function useShortcutTitle(): (label: string, id: CommandId) => string {
  const keys = useShortcutLabel();
  const t = useT();
  return useCallback(
    (label: string, id: CommandId) => {
      const bound = keys(id);
      return bound ? t("keymap.withKeys", { label, keys: bound }) : label;
    },
    [keys, t],
  );
}
