import { useT } from "../i18n";
import type { Pane, Workspace } from "../types";

/**
 * The column on the right: everything about the focused pane — its session,
 * context and cost, the board task it is on, the prompts waiting for it, the
 * files it touched and what it did lately. Reads only: lib/paneInsight for
 * what the pane reported, lib/paneTask for the board.
 */
export interface InspectorProps {
  workspace: Workspace;
  /** Every workspace, to tell which board assignee is which pane. */
  workspaces: Workspace[];
  activeWorkspaceId: string | null;
  /** null when the workspace has no pane in focus. */
  pane: Pane | null;
  /** 1-based position in reading order, for the "focus pane N" key. */
  paneIndex: number | null;
  width: number;
  onResize: (width: number) => void;
  /** Section ids folded away (Settings.inspectorCollapsed). */
  collapsed: string[];
  onToggleSection: (id: string) => void;
  onClose: () => void;
  onOpenBoard: () => void;
  onOpenSessions: (paneId: string) => void;
}

export function Inspector({ pane, width }: InspectorProps) {
  const t = useT();
  return (
    <aside className="inspector" style={{ width }} aria-label={t("cmd.view.inspector")}>
      {pane ? <h2 className="inspector__name">{pane.name}</h2> : null}
    </aside>
  );
}
