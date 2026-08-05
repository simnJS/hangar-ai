import type { Terminal } from "@xterm/xterm";

/**
 * The live xterm instance behind each pane id.
 *
 * Shortcuts that act on the terminal itself — clear, copy, jump to the top of
 * the scrollback — need the object, not a React prop: they are dispatched once,
 * from the app, for whichever pane happens to be focused. A pane registers on
 * mount and drops out when its PTY dies with it.
 */
const terminals = new Map<string, Terminal>();

export function registerTerminal(id: string, term: Terminal): () => void {
  terminals.set(id, term);
  // Guarded: a pane respawned under the same id would otherwise have its fresh
  // terminal removed by the previous one's cleanup.
  return () => {
    if (terminals.get(id) === term) terminals.delete(id);
  };
}

export const getTerminal = (id: string | null): Terminal | null =>
  (id && terminals.get(id)) || null;
