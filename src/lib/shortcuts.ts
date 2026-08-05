import {
  formatBinding,
  parseBinding,
  serializeBinding,
  serializeChords,
  type Binding,
  type Chord,
} from "./keys";
import type { TranslateKey, TranslateParams, Translator } from "../i18n";

/**
 * Every keystroke the app answers to, in one place.
 *
 * Commands are the stable half — code refers to `"pane.split"`, never to a key
 * — and bindings are the part the user owns. Settings only ever store what
 * differs from the defaults below, so a default that changes in a later version
 * reaches everyone who never touched it.
 */

export type ShortcutSection =
  | "panes"
  | "navigation"
  | "workspaces"
  | "view"
  | "terminal"
  | "app";

export const SECTIONS: ShortcutSection[] = [
  "panes",
  "navigation",
  "workspaces",
  "view",
  "terminal",
  "app",
];

type Digit = 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9;

export type CommandId =
  | "pane.split"
  | "pane.splitRight"
  | "pane.splitDown"
  | "pane.close"
  | "pane.restart"
  | "pane.restartAll"
  | "pane.sessions"
  | "pane.next"
  | "pane.prev"
  | "pane.focusLeft"
  | "pane.focusRight"
  | "pane.focusUp"
  | "pane.focusDown"
  | `pane.focus${Digit}`
  | "pane.swapLeft"
  | "pane.swapRight"
  | "pane.swapUp"
  | "pane.swapDown"
  | "workspace.next"
  | "workspace.prev"
  | "workspace.new"
  | `workspace.go${Digit}`
  | "view.settings"
  | "view.shortcuts"
  | "view.terminals"
  | "view.board"
  | "view.mcp"
  | "view.broadcast"
  | "terminal.clear"
  | "terminal.copy"
  | "terminal.paste"
  | "terminal.selectAll"
  | "terminal.scrollTop"
  | "terminal.scrollBottom"
  | "app.zoomIn"
  | "app.zoomOut"
  | "app.zoomReset"
  | "app.fullscreen";

export interface ShortcutCommand {
  id: CommandId;
  section: ShortcutSection;
  labelKey: TranslateKey;
  /** For the generated families, where one label covers nine commands. */
  labelParams?: TranslateParams;
  defaults: Binding[];
}

const DIGITS: Digit[] = [1, 2, 3, 4, 5, 6, 7, 8, 9];

const focusDigits: ShortcutCommand[] = DIGITS.map((n) => ({
  id: `pane.focus${n}` as CommandId,
  section: "navigation",
  labelKey: "cmd.pane.focusN",
  labelParams: { n },
  defaults: [`Ctrl+${n}`],
}));

const workspaceDigits: ShortcutCommand[] = DIGITS.map((n) => ({
  id: `workspace.go${n}` as CommandId,
  section: "workspaces",
  labelKey: "cmd.workspace.goN",
  labelParams: { n },
  defaults: [`Alt+${n}`],
}));

/**
 * Defaults lean on Ctrl+Shift for pane management and plain Alt for moving
 * around, which keeps every Ctrl+letter the shell needs — Ctrl+C, Ctrl+D,
 * Ctrl+R — reaching the terminal untouched.
 */
export const COMMANDS: ShortcutCommand[] = [
  {
    id: "pane.split",
    section: "panes",
    labelKey: "cmd.pane.split",
    defaults: ["Ctrl+Shift+Enter"],
  },
  {
    id: "pane.splitRight",
    section: "panes",
    labelKey: "cmd.pane.splitRight",
    defaults: ["Ctrl+Alt+Right"],
  },
  {
    id: "pane.splitDown",
    section: "panes",
    labelKey: "cmd.pane.splitDown",
    defaults: ["Ctrl+Alt+Down"],
  },
  {
    id: "pane.close",
    section: "panes",
    labelKey: "cmd.pane.close",
    defaults: ["Ctrl+Shift+X"],
  },
  {
    id: "pane.restart",
    section: "panes",
    labelKey: "cmd.pane.restart",
    defaults: ["Ctrl+Shift+R"],
  },
  {
    id: "pane.restartAll",
    section: "panes",
    labelKey: "cmd.pane.restartAll",
    defaults: ["Ctrl+Alt+Shift+R"],
  },
  {
    id: "pane.sessions",
    section: "panes",
    labelKey: "cmd.pane.sessions",
    defaults: ["Ctrl+Shift+S"],
  },

  {
    id: "pane.next",
    section: "navigation",
    labelKey: "cmd.pane.next",
    defaults: ["Ctrl+Tab"],
  },
  {
    id: "pane.prev",
    section: "navigation",
    labelKey: "cmd.pane.prev",
    defaults: ["Ctrl+Shift+Tab"],
  },
  {
    id: "pane.focusLeft",
    section: "navigation",
    labelKey: "cmd.pane.focusLeft",
    defaults: ["Alt+Left"],
  },
  {
    id: "pane.focusRight",
    section: "navigation",
    labelKey: "cmd.pane.focusRight",
    defaults: ["Alt+Right"],
  },
  {
    id: "pane.focusUp",
    section: "navigation",
    labelKey: "cmd.pane.focusUp",
    defaults: ["Alt+Up"],
  },
  {
    id: "pane.focusDown",
    section: "navigation",
    labelKey: "cmd.pane.focusDown",
    defaults: ["Alt+Down"],
  },
  ...focusDigits,
  {
    id: "pane.swapLeft",
    section: "navigation",
    labelKey: "cmd.pane.swapLeft",
    defaults: ["Alt+Shift+Left"],
  },
  {
    id: "pane.swapRight",
    section: "navigation",
    labelKey: "cmd.pane.swapRight",
    defaults: ["Alt+Shift+Right"],
  },
  {
    id: "pane.swapUp",
    section: "navigation",
    labelKey: "cmd.pane.swapUp",
    defaults: ["Alt+Shift+Up"],
  },
  {
    id: "pane.swapDown",
    section: "navigation",
    labelKey: "cmd.pane.swapDown",
    defaults: ["Alt+Shift+Down"],
  },

  {
    id: "workspace.next",
    section: "workspaces",
    labelKey: "cmd.workspace.next",
    defaults: ["Ctrl+PageDown"],
  },
  {
    id: "workspace.prev",
    section: "workspaces",
    labelKey: "cmd.workspace.prev",
    defaults: ["Ctrl+PageUp"],
  },
  {
    id: "workspace.new",
    section: "workspaces",
    labelKey: "cmd.workspace.new",
    defaults: ["Ctrl+Shift+N"],
  },
  ...workspaceDigits,

  {
    id: "view.settings",
    section: "view",
    labelKey: "cmd.view.settings",
    defaults: ["Ctrl+,"],
  },
  {
    id: "view.shortcuts",
    section: "view",
    labelKey: "cmd.view.shortcuts",
    // A two-step sequence, both because it reads well and because it is the
    // one place the feature documents itself.
    defaults: ["Ctrl+K Ctrl+S"],
  },
  {
    id: "view.terminals",
    section: "view",
    labelKey: "cmd.view.terminals",
    defaults: ["Ctrl+Alt+T"],
  },
  {
    id: "view.board",
    section: "view",
    labelKey: "cmd.view.board",
    defaults: ["Ctrl+Alt+B"],
  },
  {
    id: "view.mcp",
    section: "view",
    labelKey: "cmd.view.mcp",
    defaults: ["Ctrl+Alt+M"],
  },
  {
    id: "view.broadcast",
    section: "view",
    labelKey: "cmd.view.broadcast",
    defaults: ["Ctrl+Shift+B"],
  },

  {
    id: "terminal.clear",
    section: "terminal",
    labelKey: "cmd.terminal.clear",
    defaults: ["Ctrl+Shift+L"],
  },
  {
    id: "terminal.copy",
    section: "terminal",
    labelKey: "cmd.terminal.copy",
    defaults: ["Ctrl+Shift+C"],
  },
  {
    id: "terminal.paste",
    section: "terminal",
    labelKey: "cmd.terminal.paste",
    defaults: ["Ctrl+Shift+V"],
  },
  {
    id: "terminal.selectAll",
    section: "terminal",
    labelKey: "cmd.terminal.selectAll",
    defaults: ["Ctrl+Shift+A"],
  },
  {
    id: "terminal.scrollTop",
    section: "terminal",
    labelKey: "cmd.terminal.scrollTop",
    defaults: ["Ctrl+Home"],
  },
  {
    id: "terminal.scrollBottom",
    section: "terminal",
    labelKey: "cmd.terminal.scrollBottom",
    defaults: ["Ctrl+End"],
  },

  {
    id: "app.zoomIn",
    section: "app",
    labelKey: "cmd.app.zoomIn",
    // Both spellings of the same physical key, because a keyboard that needs
    // Shift for "+" would otherwise report the shifted chord only.
    defaults: ["Ctrl+=", "Ctrl++"],
  },
  {
    id: "app.zoomOut",
    section: "app",
    labelKey: "cmd.app.zoomOut",
    defaults: ["Ctrl+-"],
  },
  {
    id: "app.zoomReset",
    section: "app",
    labelKey: "cmd.app.zoomReset",
    defaults: ["Ctrl+0"],
  },
  {
    id: "app.fullscreen",
    section: "app",
    labelKey: "cmd.app.fullscreen",
    defaults: ["F11"],
  },
];

export const COMMANDS_BY_ID = new Map(COMMANDS.map((command) => [command.id, command]));

/**
 * What the settings store: only the commands the user re-bound. An empty array
 * is meaningful — it is how a command is turned off — so it is kept as is.
 */
export type Keymap = Record<string, Binding[]>;

/** Every command's bindings, defaults filled in. */
export function resolveKeymap(overrides: Keymap | undefined | null): Keymap {
  const resolved: Keymap = {};
  for (const command of COMMANDS) {
    const custom = overrides?.[command.id];
    resolved[command.id] = Array.isArray(custom) ? custom : command.defaults;
  }
  return resolved;
}

const sameList = (a: Binding[], b: Binding[]) =>
  a.length === b.length &&
  a.every((binding, i) => serializeBinding(binding) === serializeBinding(b[i]));

/** Drops overrides that ended up matching the default again. */
function prune(overrides: Keymap): Keymap {
  const next: Keymap = {};
  for (const [id, bindings] of Object.entries(overrides)) {
    const command = COMMANDS_BY_ID.get(id as CommandId);
    // Unknown ids are kept: they belong to a version that knows more commands
    // than this one, and dropping them would lose that user's bindings.
    if (command && sameList(command.defaults, bindings)) continue;
    next[id] = bindings;
  }
  return next;
}

export const bindingsOf = (keymap: Keymap, id: CommandId): Binding[] =>
  keymap[id] ?? COMMANDS_BY_ID.get(id)?.defaults ?? [];

/** Adds a binding to a command, ignoring one it already answers to. */
export function withBinding(
  overrides: Keymap,
  keymap: Keymap,
  id: CommandId,
  binding: Binding,
): Keymap {
  const serial = serializeBinding(binding);
  if (!serial) return overrides;
  const current = bindingsOf(keymap, id);
  if (current.some((entry) => serializeBinding(entry) === serial)) return overrides;
  return prune({ ...overrides, [id]: [...current, binding] });
}

export function withoutBinding(
  overrides: Keymap,
  keymap: Keymap,
  id: CommandId,
  binding: Binding,
): Keymap {
  const serial = serializeBinding(binding);
  const current = bindingsOf(keymap, id);
  return prune({
    ...overrides,
    [id]: current.filter((entry) => serializeBinding(entry) !== serial),
  });
}

/** Puts a command back on its shipped bindings. */
export function resetCommand(overrides: Keymap, id: CommandId): Keymap {
  const next = { ...overrides };
  delete next[id];
  return next;
}

export const isCustomized = (overrides: Keymap, id: CommandId) => id in overrides;

export interface KeymapIndex {
  /** Serialized sequence → the commands listening for it. */
  exact: Map<string, CommandId[]>;
  /** Serialized prefixes of longer sequences: the dispatcher waits on these. */
  prefixes: Set<string>;
}

export function indexKeymap(keymap: Keymap): KeymapIndex {
  const exact = new Map<string, CommandId[]>();
  const prefixes = new Set<string>();

  for (const command of COMMANDS) {
    for (const binding of bindingsOf(keymap, command.id)) {
      const chords = parseBinding(binding);
      // A binding that no longer parses — hand-edited state, a version that
      // spelled keys differently — is skipped rather than crashing the app.
      if (!chords) continue;
      const serial = serializeChords(chords);
      const listeners = exact.get(serial);
      if (listeners) listeners.push(command.id);
      else exact.set(serial, [command.id]);

      for (let i = 1; i < chords.length; i++) {
        prefixes.add(serializeChords(chords.slice(0, i)));
      }
    }
  }

  return { exact, prefixes };
}

export type ConflictKind =
  /** Several commands on the same keystroke: only the first one ever runs. */
  | "duplicate"
  /** A sequence whose first chord is already a shortcut of its own. */
  | "shadow";

export interface Conflict {
  kind: ConflictKind;
  binding: Binding;
  commands: CommandId[];
}

export function findConflicts(keymap: Keymap): Conflict[] {
  const { exact } = indexKeymap(keymap);
  const conflicts: Conflict[] = [];

  for (const [serial, commands] of exact) {
    if (commands.length > 1) {
      conflicts.push({ kind: "duplicate", binding: displayOf(keymap, commands[0], serial), commands });
    }
  }

  // A sequence is unreachable when its opening chord already fires something.
  for (const [serial, commands] of exact) {
    if (!serial.includes(" ")) continue;
    const head = serial.split(" ")[0];
    const owners = exact.get(head);
    if (!owners) continue;
    conflicts.push({
      kind: "shadow",
      binding: displayOf(keymap, commands[0], serial),
      commands: [...owners, ...commands],
    });
  }

  return conflicts;
}

/** The written form a command uses for a given serialized sequence. */
function displayOf(keymap: Keymap, id: CommandId, serial: string): Binding {
  for (const binding of bindingsOf(keymap, id)) {
    if (serializeBinding(binding) === serial) return binding;
  }
  return serial;
}

/** Commands already answering to a keystroke, `id` aside. */
export function commandsFor(
  keymap: Keymap,
  chords: Chord[],
  except?: CommandId,
): CommandId[] {
  const serial = serializeChords(chords);
  const owners = indexKeymap(keymap).exact.get(serial) ?? [];
  return owners.filter((id) => id !== except);
}

export const commandLabel = (t: Translator, command: ShortcutCommand) =>
  t(command.labelKey, command.labelParams);

/** First binding of a command, formatted for a tooltip. Empty when unbound. */
export function shortcutLabel(keymap: Keymap, id: CommandId): string {
  const [first] = bindingsOf(keymap, id);
  return first ? formatBinding(first) : "";
}
