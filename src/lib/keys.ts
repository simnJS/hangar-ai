/**
 * Keystroke algebra.
 *
 * One shape, `Chord`, for both what the user just pressed and what the settings
 * hold, so matching a keystroke against a binding is a comparison rather than a
 * parsing job. Everything here is pure — the dispatcher lives in useShortcuts.
 */

export interface Chord {
  /** Canonical key name: "A", "1", "Enter", "Up", "," — never a modifier. */
  key: string;
  ctrl: boolean;
  shift: boolean;
  alt: boolean;
  meta: boolean;
}

/**
 * One or more chords typed in a row, written the way the settings store them:
 * `"Ctrl+Shift+Enter"`, or `"Ctrl+K Ctrl+S"` for a two-step sequence.
 */
export type Binding = string;

const MAC = /mac|iphone|ipad/i.test(
  typeof navigator === "undefined" ? "" : `${navigator.platform} ${navigator.userAgent}`,
);

export const isMac = () => MAC;

/** Modifier spellings accepted in a written binding. */
const MODIFIERS: Record<string, keyof Omit<Chord, "key">> = {
  ctrl: "ctrl",
  control: "ctrl",
  ctl: "ctrl",
  shift: "shift",
  alt: "alt",
  option: "alt",
  opt: "alt",
  meta: "meta",
  cmd: "meta",
  command: "meta",
  super: "meta",
  win: "meta",
};

/** Written in defaults to mean "Cmd on a Mac, Ctrl everywhere else". */
const PLATFORM_MODIFIER = "mod";

/** Spellings folded onto one canonical key name. */
const KEY_ALIASES: Record<string, string> = {
  esc: "Escape",
  escape: "Escape",
  enter: "Enter",
  return: "Enter",
  space: "Space",
  spacebar: "Space",
  " ": "Space",
  tab: "Tab",
  backspace: "Backspace",
  del: "Delete",
  delete: "Delete",
  ins: "Insert",
  insert: "Insert",
  home: "Home",
  end: "End",
  pgup: "PageUp",
  pageup: "PageUp",
  pgdn: "PageDown",
  pagedown: "PageDown",
  up: "Up",
  arrowup: "Up",
  down: "Down",
  arrowdown: "Down",
  left: "Left",
  arrowleft: "Left",
  right: "Right",
  arrowright: "Right",
  plus: "+",
  minus: "-",
  comma: ",",
  period: ".",
  slash: "/",
  backslash: "\\",
};

/** Keys that only ever qualify another one; a chord can never be made of them. */
const BARE_MODIFIER_KEYS = new Set([
  "Control",
  "Shift",
  "Alt",
  "Meta",
  "AltGraph",
  "CapsLock",
  "NumLock",
  "ScrollLock",
  "OS",
  "Dead",
  "Unidentified",
]);

const ARROW_SYMBOLS: Record<string, string> = {
  Up: "↑",
  Down: "↓",
  Left: "←",
  Right: "→",
};

/** Folds any spelling of a key onto the one name used everywhere else. */
export function canonicalKey(raw: string): string | null {
  const text = raw.trim();
  if (!text) return null;

  const lower = text.toLowerCase();
  const alias = KEY_ALIASES[lower];
  if (alias) return alias;
  if (/^f\d{1,2}$/.test(lower)) return lower.toUpperCase();
  if (text.length === 1) return /[a-z]/i.test(text) ? text.toUpperCase() : text;

  // Anything else keeps its DOM spelling, capitalised ("Home", "F13"…).
  return text.charAt(0).toUpperCase() + text.slice(1);
}

/**
 * Splits on "+", except for a "+" that is the key itself: `"Ctrl++"` is three
 * plus signs' worth of ambiguity that a plain `split("+")` gets wrong.
 */
function tokenize(text: string): string[] {
  const tokens: string[] = [];
  let buffer = "";
  for (const char of text) {
    if (char === "+" && buffer.trim()) {
      tokens.push(buffer.trim());
      buffer = "";
    } else {
      buffer += char;
    }
  }
  if (buffer.trim()) tokens.push(buffer.trim());
  return tokens;
}

/** `null` for anything that is not a whole chord — a lone modifier included. */
export function parseChord(text: string): Chord | null {
  const tokens = tokenize(text.trim());
  if (!tokens.length) return null;

  const chord: Chord = { key: "", ctrl: false, shift: false, alt: false, meta: false };

  for (let i = 0; i < tokens.length - 1; i++) {
    const name = tokens[i].toLowerCase();
    if (name === PLATFORM_MODIFIER) {
      chord[MAC ? "meta" : "ctrl"] = true;
      continue;
    }
    const modifier = MODIFIERS[name];
    // An unknown word where a modifier belongs makes the whole thing unusable:
    // guessing would silently bind a key the user never asked for.
    if (!modifier) return null;
    chord[modifier] = true;
  }

  const key = canonicalKey(tokens[tokens.length - 1]);
  if (!key || MODIFIERS[key.toLowerCase()] || key.toLowerCase() === PLATFORM_MODIFIER) {
    return null;
  }
  return { ...chord, key };
}

/** `null` as soon as one chord of the sequence does not parse. */
export function parseBinding(binding: Binding): Chord[] | null {
  const parts = binding.trim().split(/\s+/).filter(Boolean);
  if (!parts.length) return null;
  const chords: Chord[] = [];
  for (const part of parts) {
    const chord = parseChord(part);
    if (!chord) return null;
    chords.push(chord);
  }
  return chords;
}

/**
 * The comparable form of a chord: same modifiers in the same order, always.
 * Used as a map key, never shown — `chordSegments` is what the eye reads.
 */
export function serializeChord(chord: Chord): string {
  return (
    (chord.ctrl ? "ctrl+" : "") +
    (chord.alt ? "alt+" : "") +
    (chord.shift ? "shift+" : "") +
    (chord.meta ? "meta+" : "") +
    chord.key
  );
}

export const serializeChords = (chords: Chord[]) => chords.map(serializeChord).join(" ");

/** `null` when the binding does not parse, so callers can drop it. */
export function serializeBinding(binding: Binding): string | null {
  const chords = parseBinding(binding);
  return chords ? serializeChords(chords) : null;
}

/** The pieces of a chord, in the order this platform writes them. */
export function chordSegments(chord: Chord): string[] {
  const key = ARROW_SYMBOLS[chord.key] ?? chord.key;
  if (MAC) {
    return [
      ...(chord.ctrl ? ["⌃"] : []),
      ...(chord.alt ? ["⌥"] : []),
      ...(chord.shift ? ["⇧"] : []),
      ...(chord.meta ? ["⌘"] : []),
      key,
    ];
  }
  return [
    ...(chord.ctrl ? ["Ctrl"] : []),
    ...(chord.alt ? ["Alt"] : []),
    ...(chord.shift ? ["Shift"] : []),
    ...(chord.meta ? ["Win"] : []),
    key,
  ];
}

export const formatChord = (chord: Chord) => chordSegments(chord).join(MAC ? "" : "+");

/** Human form of a whole binding, sequences included: `"Ctrl+K Ctrl+S"`. */
export function formatBinding(binding: Binding): string {
  const chords = parseBinding(binding);
  if (!chords) return binding;
  return chords.map(formatChord).join(" ");
}

/** Canonical written form, which is what gets stored once a key is recorded. */
export function writeChord(chord: Chord): Binding {
  return [
    ...(chord.ctrl ? ["Ctrl"] : []),
    ...(chord.alt ? ["Alt"] : []),
    ...(chord.shift ? ["Shift"] : []),
    ...(chord.meta ? ["Meta"] : []),
    chord.key,
  ].join("+");
}

export const writeBinding = (chords: Chord[]): Binding => chords.map(writeChord).join(" ");

const digitFromCode = (code: string): string | null => {
  const match = /^(?:Digit|Numpad)(\d)$/.exec(code);
  return match ? match[1] : null;
};

const letterFromCode = (code: string): string | null =>
  /^Key[A-Z]$/.test(code) ? code.slice(3) : null;

/** Numpad keys that carry a symbol rather than a digit. */
const NUMPAD_CODES: Record<string, string> = {
  NumpadAdd: "+",
  NumpadSubtract: "-",
  NumpadMultiply: "*",
  NumpadDivide: "/",
  NumpadDecimal: ".",
  NumpadEnter: "Enter",
};

/**
 * The key of an event, named the way the settings name it.
 *
 * `event.key` leads, because it is the character printed on the key the user
 * actually pressed — on an AZERTY board `event.code` for the "A" key reads
 * `KeyQ`, and binding by code would put the shortcut on the wrong key. Two
 * cases fall back to the code: a digit row that needs Shift to type digits
 * (AZERTY again — Ctrl+1 has to stay Ctrl+1), and a non-Latin layout, whose
 * `event.key` is a character no binding could ever be written with.
 */
export function keyFromEvent(event: KeyboardEvent): string | null {
  const named = NUMPAD_CODES[event.code];
  if (named) return named;

  const key = event.key;
  if (!key || BARE_MODIFIER_KEYS.has(key)) return null;

  if (key.length === 1) {
    if (/[a-z]/i.test(key)) return key.toUpperCase();
    if (/[0-9]/.test(key)) return key;
    const digit = digitFromCode(event.code);
    if (digit) return digit;
    const letter = letterFromCode(event.code);
    if (letter) return letter;
    return key === " " ? "Space" : key;
  }

  return canonicalKey(key);
}

/**
 * `null` when the event is not a chord: a modifier pressed on its own, or a
 * character being composed with AltGr — which Windows reports as Ctrl+Alt, so
 * without this every `@` typed on an AZERTY board would fire Ctrl+Alt+0.
 */
export function chordFromEvent(event: KeyboardEvent): Chord | null {
  if (event.ctrlKey && event.altKey && event.getModifierState?.("AltGraph")) return null;
  const key = keyFromEvent(event);
  if (!key) return null;
  return {
    key,
    ctrl: event.ctrlKey,
    shift: event.shiftKey,
    alt: event.altKey,
    meta: event.metaKey,
  };
}

/** True when a chord carries no modifier that would keep it out of typing. */
export function isBareChord(chord: Chord): boolean {
  return !chord.ctrl && !chord.alt && !chord.meta;
}

/** Letters the platform modifier reserves for editing a text field. */
const EDITING_KEYS = new Set(["A", "C", "V", "X", "Z", "Y"]);

/**
 * True for the chords a text field owns — Cmd+C on a Mac, Ctrl+C elsewhere.
 *
 * Nothing is bound to them by default, but they are the obvious thing to
 * rebind copy and paste onto, and the dispatcher captures above every field in
 * the app. Without this, doing so would take copying away from the workspace
 * name, the broadcast box and the task board at the same time.
 */
export function isTextEditingChord(chord: Chord): boolean {
  if (chord.alt || !EDITING_KEYS.has(chord.key)) return false;
  return MAC ? chord.meta && !chord.ctrl : chord.ctrl && !chord.meta;
}
