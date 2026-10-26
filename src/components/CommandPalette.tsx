import type { Keymap } from "../lib/shortcuts";
import type { ShortcutHandlers } from "../lib/useShortcuts";
import type { Workspace } from "../types";

/**
 * Every command the app can run right now, its panes and its workspaces,
 * behind one search field. The commands are the ones the keyboard shortcuts
 * dispatch at this moment (App.buildHandlers), labelled with their
 * `cmd.*` strings, so the palette never offers what a key would refuse.
 */
export interface CommandPaletteProps {
  onClose: () => void;
  /** What each command does right now; a command absent here is unavailable. */
  handlers: ShortcutHandlers;
  keymap: Keymap;
  /** In sidebar order. */
  workspaces: Workspace[];
  activeWorkspaceId: string | null;
  focusedPaneId: string | null;
  onFocusPane: (workspaceId: string, paneId: string) => void;
  onActivateWorkspace: (workspaceId: string) => void;
}

export function CommandPalette(_props: CommandPaletteProps) {
  return null;
}
