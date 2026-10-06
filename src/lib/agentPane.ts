import type { Workspace } from "../types";

/**
 * Which pane an agent signing the board as `assignee` runs in.
 *
 * The board only knows the free-form name an agent chose when it claimed, and
 * nothing ties that name to a pane. So this guesses, and only answers when the
 * guess is unambiguous — focusing the wrong agent is worse than offering no
 * shortcut at all:
 *
 * 1. a workspace and one of its panes, in either order — "hangar-ava", or
 *    "Ava (Hangar)", the name a Claude Code session runs under — which also
 *    tells apart two panes a board from before names were unique names alike;
 * 2. a pane whose name is in the assignee ("Ava", "claude:ava") — in the
 *    active workspace first, then in any other, since pane names are unique
 *    across workspaces; the longest name wins, so "Ava 2" beats "Ava";
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

  const named = (candidates: Workspace[]): PaneTarget | null => {
    let best: (PaneTarget & { length: number })[] = [];
    for (const ws of candidates) {
      for (const pane of ws.panes) {
        const name = words(pane.name);
        if (!containsRun(said, name)) continue;
        if (!best.length || name.length > best[0].length) best = [];
        if (!best.length || name.length === best[0].length) {
          best.push({ workspaceId: ws.id, paneId: pane.id, length: name.length });
        }
      }
    }
    return best.length === 1 ? { workspaceId: best[0].workspaceId, paneId: best[0].paneId } : null;
  };

  for (const ws of workspaces) {
    const prefix = words(ws.name);
    if (!prefix.length) continue;
    // Longest name first, so "Ava 2" is not taken for "Ava".
    const panes = [...ws.panes].sort((a, b) => words(b.name).length - words(a.name).length);
    for (const pane of panes) {
      const name = words(pane.name);
      if (containsRun(said, [...prefix, ...name]) || containsRun(said, [...name, ...prefix])) {
        return { workspaceId: ws.id, paneId: pane.id };
      }
    }
  }

  const here = active ? named([active]) : null;
  if (here) return here;
  const anywhere = named(workspaces);
  if (anywhere) return anywhere;

  if (active) {
    const running = active.panes.filter(
      (pane) => pane.agent !== "shell" && said.includes(pane.agent),
    );
    if (running.length === 1) return { workspaceId: active.id, paneId: running[0].id };
  }

  return null;
}
