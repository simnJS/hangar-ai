import type { CommandId } from "./shortcuts";

/**
 * How the command palette matches what is typed against what it offers.
 *
 * Forgiving where people are sloppy — accents and case, so "memoire" finds
 * "Mémoire" — and strict about order where it says something: a label that
 * starts with the query beats one with a word starting with it, which beats
 * one merely containing it. Several words find a label holding all of them,
 * in any order. Pure, so the palette's ranking can be tested without a DOM.
 */

/**
 * The other words people reach for when they look for a command, in both
 * languages at once: someone who thinks "paramètres" or "preferences" is
 * looking for "Réglages". The palette also indexes each command's label in
 * the other language, so these only cover what neither label says. Like the
 * agent and group keywords, they rank below every label match.
 */
export const COMMAND_SYNONYMS: Partial<Record<CommandId, readonly string[]>> = {
  "view.settings": ["paramètres", "préférences", "options", "configuration", "preferences", "config"],
  "view.shortcuts": ["clavier", "touches", "keyboard", "keybindings", "hotkeys"],
  "view.terminals": ["grille", "grid"],
  "view.board": ["kanban", "tâches", "tasks", "todo"],
  "view.boardDock": ["kanban", "tâches", "tasks", "dock"],
  "view.modelPreview": ["modèle", "model", "maillage", "mesh", "glb", "gltf", "fbx", "obj", "viewer"],
  "view.closeModels": ["modèle", "model", "3d"],
  "view.memory": ["notes", "souvenirs", "knowledge"],
  "view.mcp": ["serveur", "server", "connecter", "connect", "intégration", "integration"],
  "workspace.mcpServers": ["mémoire", "memory", "processus", "process", "ram", "strict"],
  "view.broadcast": ["diffusion", "envoyer à tous", "send to all", "everyone"],
  "view.navigator": ["barre latérale", "sidebar", "arborescence", "tree"],
  "view.inspector": ["détails", "details", "infos", "info"],
  "pane.split": ["nouveau terminal", "new terminal", "ajouter", "add", "ouvrir", "open"],
  "pane.splitRight": ["vertical", "côte à côte", "side by side"],
  "pane.splitDown": ["horizontal"],
  "pane.close": ["supprimer", "remove", "kill", "quitter", "quit"],
  "pane.restart": ["redémarrer", "relaunch", "reload"],
  "pane.restartAll": ["redémarrer", "relaunch", "reload"],
  "pane.resetAll": ["conversation", "vider", "effacer", "clear", "remise à zéro"],
  "pane.tidy": ["ranger", "aligner", "grille", "disposition", "arrange", "grid", "layout"],
  "pane.sessions": ["reprendre", "historique", "conversation", "resume", "history", "transcript"],
  "pane.zoom": ["agrandir", "maximiser", "plein écran", "maximize", "focus"],
  "workspace.new": ["projet", "dossier", "créer", "ajouter", "project", "folder", "create", "add"],
  "terminal.clear": ["cls", "vider", "nettoyer", "reset"],
  "terminal.find": ["chercher", "trouver", "find", "grep"],
  "app.zoomIn": ["taille", "plus grand", "size", "larger"],
  "app.zoomOut": ["taille", "plus petit", "size", "smaller"],
  "app.fullscreen": ["plein écran", "maximiser", "maximize"],
};

/** The synonyms of a command, or none. */
export const commandSynonyms = (id: CommandId): readonly string[] => COMMAND_SYNONYMS[id] ?? [];

/** `[start, end)` in the original, unfolded text: what to highlight. */
export interface MatchRange {
  start: number;
  end: number;
}

export interface Match {
  /** Higher is better; 0 when there is no query at all. */
  score: number;
  ranges: MatchRange[];
}

/** The label starts with the query. */
export const SCORE_PREFIX = 4;
/** A word of the label starts with the query. */
export const SCORE_WORD = 3;
/** The query sits somewhere inside the label. */
export const SCORE_SUBSTRING = 2;
/** Every word of the query is in the label, each at the start of a word. */
export const SCORE_WORDS = 1.5;
/** Every word of the query is in the label, somewhere. */
export const SCORE_TOKENS = 1;
/** Only the secondary text matched — the agent, the group. Nothing to mark. */
export const SCORE_KEYWORD = 0.5;

const COMBINING_MARKS = /\p{M}/gu;
const LETTER_OR_DIGIT = /[\p{L}\p{N}]/u;
/** The no-break spaces French labels carry (lib/typography), one for one. */
const NO_BREAK_SPACES = /[  ]/g;

/**
 * Accents, case and no-break spaces folded away, the way both sides are
 * compared: what is typed has plain spaces, "20 h 01" in a label does not.
 */
export function fold(text: string): string {
  return text
    .normalize("NFD")
    .replace(COMBINING_MARKS, "")
    .replace(NO_BREAK_SPACES, " ")
    .toLowerCase();
}

/**
 * The folded text, plus where each of its characters came from.
 *
 * Folding changes lengths — "é" decomposes into two code units before the
 * accent is dropped, and a few capitals lower into two — so an index in the
 * folded text is not an index in the original. Folding one character at a time
 * keeps the way back, which is what lets the palette bold the right letters.
 */
function foldWithOrigins(text: string): { folded: string; starts: number[]; ends: number[] } {
  let folded = "";
  const starts: number[] = [];
  const ends: number[] = [];
  let at = 0;
  for (const char of text) {
    const piece = fold(char);
    for (let i = 0; i < piece.length; i++) {
      starts.push(at);
      ends.push(at + char.length);
    }
    folded += piece;
    at += char.length;
  }
  return { folded, starts, ends };
}

/** First index where `needle` starts a word of `haystack`, or -1. */
function wordStart(haystack: string, needle: string): number {
  let from = haystack.indexOf(needle);
  while (from >= 0) {
    if (from === 0 || !LETTER_OR_DIGIT.test(haystack[from - 1])) return from;
    from = haystack.indexOf(needle, from + 1);
  }
  return -1;
}

/** Sorted, and overlapping or touching ranges merged into one. */
function mergeRanges(ranges: MatchRange[]): MatchRange[] {
  const sorted = [...ranges].sort((a, b) => a.start - b.start);
  const merged: MatchRange[] = [];
  for (const range of sorted) {
    const last = merged[merged.length - 1];
    if (last && range.start <= last.end) last.end = Math.max(last.end, range.end);
    else merged.push({ ...range });
  }
  return merged;
}

/** The query as it is compared: folded, trimmed, single-spaced. */
export const foldQuery = (query: string) => fold(query).trim().replace(/\s+/g, " ");

/**
 * How well `text` answers `query`, or null when it does not. An empty query
 * matches everything with a score of 0.
 */
export function matchText(text: string, query: string): Match | null {
  const q = foldQuery(query);
  if (!q) return { score: 0, ranges: [] };

  const { folded, starts, ends } = foldWithOrigins(text);
  const span = (at: number, length: number): MatchRange => ({
    start: starts[at],
    end: ends[at + length - 1],
  });

  if (folded.startsWith(q)) return { score: SCORE_PREFIX, ranges: [span(0, q.length)] };
  const word = wordStart(folded, q);
  if (word >= 0) return { score: SCORE_WORD, ranges: [span(word, q.length)] };
  const inside = folded.indexOf(q);
  if (inside >= 0) return { score: SCORE_SUBSTRING, ranges: [span(inside, q.length)] };

  const tokens = q.split(" ");
  if (tokens.length < 2) return null;
  const ranges: MatchRange[] = [];
  let atWordStart = 0;
  for (const token of tokens) {
    const start = wordStart(folded, token);
    const at = start >= 0 ? start : folded.indexOf(token);
    if (at < 0) return null;
    if (start >= 0) atWordStart += 1;
    ranges.push(span(at, token.length));
  }
  return {
    score: atWordStart === tokens.length ? SCORE_WORDS : SCORE_TOKENS,
    ranges: mergeRanges(ranges),
  };
}

/**
 * The label decides; the keywords — the agent a pane runs, the group it is
 * filed under — only rescue an entry the label alone would have missed, and
 * rank it below every label match.
 */
export function matchEntry(label: string, keywords: string[], query: string): Match | null {
  const direct = matchText(label, query);
  if (direct) return direct;
  const all = keywords.join(" ");
  return all && matchText(all, query) ? { score: SCORE_KEYWORD, ranges: [] } : null;
}

/** `text` cut into runs, each marked as matched or not, for rendering. */
export function highlight(
  text: string,
  ranges: MatchRange[],
): { text: string; hit: boolean }[] {
  const parts: { text: string; hit: boolean }[] = [];
  let at = 0;
  for (const { start, end } of mergeRanges(ranges)) {
    if (start > at) parts.push({ text: text.slice(at, start), hit: false });
    if (end > start) parts.push({ text: text.slice(start, end), hit: true });
    at = Math.max(at, end);
  }
  if (at < text.length) parts.push({ text: text.slice(at), hit: false });
  return parts;
}

export interface PaletteEntry {
  /** The heading it is listed under. Entries of a group are kept together. */
  group: string;
  label: string;
  keywords?: string[];
}

export interface PaletteGroup<T> {
  group: string;
  entries: { entry: T; match: Match }[];
}

/**
 * The entries `query` keeps, grouped under their headings.
 *
 * With nothing typed, everything in the order it was given. With a query, the
 * best entries of each group come first, and the group holding the best entry
 * overall comes first — so the top row, the one Enter runs, is always the best
 * match there is. Ties keep the order given, which is the order the palette
 * thinks most useful.
 */
export function filterPalette<T extends PaletteEntry>(
  entries: T[],
  query: string,
): PaletteGroup<T>[] {
  const groups: PaletteGroup<T>[] = [];
  const byName = new Map<string, PaletteGroup<T>>();
  for (const entry of entries) {
    const match = matchEntry(entry.label, entry.keywords ?? [], query);
    if (!match) continue;
    let group = byName.get(entry.group);
    if (!group) {
      group = { group: entry.group, entries: [] };
      byName.set(entry.group, group);
      groups.push(group);
    }
    group.entries.push({ entry, match });
  }

  if (!foldQuery(query)) return groups;

  // Array.prototype.sort is stable, which is what keeps ties in the given order.
  for (const group of groups) group.entries.sort((a, b) => b.match.score - a.match.score);
  const best = (group: PaletteGroup<T>) => group.entries[0]?.match.score ?? 0;
  return groups.sort((a, b) => best(b) - best(a));
}
