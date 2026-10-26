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

/**
 * What opens the diff view of a workspace's pane. The view is hosted by the
 * workspace's terminals, and asked for from elsewhere — the inspector — the
 * same way as the search bar below.
 */
const diffOpeners = new Map<string, (paneId: string) => void>();

export function registerDiffOpener(workspaceId: string, open: (paneId: string) => void) {
  diffOpeners.set(workspaceId, open);
  return () => {
    if (diffOpeners.get(workspaceId) === open) diffOpeners.delete(workspaceId);
  };
}

export function openDiff(workspaceId: string, paneId: string) {
  diffOpeners.get(workspaceId)?.(paneId);
}

/**
 * What opens each pane's search bar. The bar is state inside the pane, and the
 * shortcut that asks for it is dispatched from the app, the same way as above.
 */
const finders = new Map<string, () => void>();

export function registerFinder(id: string, open: () => void): () => void {
  finders.set(id, open);
  return () => {
    if (finders.get(id) === open) finders.delete(id);
  };
}

export function openFinder(id: string | null) {
  if (id) finders.get(id)?.();
}
