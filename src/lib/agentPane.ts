import type { Workspace } from "../types";

/**
 * Which pane an agent signing the board as `assignee` runs in.
 *
 * The board only knows the free-form name an agent chose when it claimed, and
 * nothing ties that name to a pane. So this guesses, and only answers when the
 * guess is unambiguous — focusing the wrong agent is worse than offering no
 * shortcut at all:
 *
 * 1. a pane of the active workspace whose name is in the assignee ("Ava",
 *    "claude:ava", "hangar-ava"); the longest name wins, so "Ava 2" beats "Ava";
 * 2. "<workspace>-<pane>" for any workspace — every workspace has an "Ava", so
 *    a bare name is never looked up outside the one on screen;
 * 3. the only pane of the active workspace running the agent the assignee
 *    names ("claude", "codex-1").
 */

export interface PaneTarget {
  workspaceId: string;
  paneId: string;
}

const words = (text: string) =>
  text
    .toLowerCase()
    .split(/[^\p{L}\p{N}]+/u)
    .filter(Boolean);

/** Whether `run` appears in `words` as consecutive entries. */
function containsRun(all: string[], run: string[]): boolean {
  if (!run.length || run.length > all.length) return false;
  for (let at = 0; at + run.length <= all.length; at++) {
    if (run.every((word, i) => all[at + i] === word)) return true;
  }
  return false;
}

export function findAgentPane(
  assignee: string,
  workspaces: Workspace[],
  activeId: string | null,
): PaneTarget | null {
  const said = words(assignee);
  if (!said.length) return null;
  const active = workspaces.find((ws) => ws.id === activeId) ?? null;

  if (active) {
    let best: { paneId: string; length: number }[] = [];
    for (const pane of active.panes) {
      const name = words(pane.name);
      if (!containsRun(said, name)) continue;
      if (!best.length || name.length > best[0].length) best = [];
      if (!best.length || name.length === best[0].length) {
        best.push({ paneId: pane.id, length: name.length });
      }
    }
    if (best.length === 1) return { workspaceId: active.id, paneId: best[0].paneId };
  }

  for (const ws of workspaces) {
    const prefix = words(ws.name);
    if (!prefix.length) continue;
    for (const pane of ws.panes) {
      if (containsRun(said, [...prefix, ...words(pane.name)])) {
        return { workspaceId: ws.id, paneId: pane.id };
      }
    }
  }

  if (active) {
    const running = active.panes.filter(
      (pane) => pane.agent !== "shell" && said.includes(pane.agent),
    );
    if (running.length === 1) return { workspaceId: active.id, paneId: running[0].id };
  }

  return null;
}
