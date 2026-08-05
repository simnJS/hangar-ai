import type { Pane, Workspace } from "../types";

/**
 * Panes are called by name rather than numbered.
 *
 * A number is a position, and positions move: split, close or drag a pane and
 * every index after it shifts, so "pane 3" means something else a second
 * later. A name is attached to the pane itself and survives all of it, which
 * is what makes it usable in a sentence — "Leo is done, Ava is still running".
 */

/** Short, unambiguous, easy to say out loud. One per pane, up to MAX_PANES. */
export const PANE_NAMES = [
  "Ava",
  "Max",
  "Leo",
  "Mia",
  "Sam",
  "Zoe",
  "Eli",
  "Nora",
  "Finn",
  "Ivy",
  "Theo",
  "Luna",
  "Jude",
  "Cleo",
  "Rex",
  "Wren",
];

/** First free name in a workspace; falls back to "Ava 2" once the list runs out. */
export function pickPaneName(taken: Iterable<string>): string {
  const used = new Set(taken);
  const free = PANE_NAMES.find((name) => !used.has(name));
  if (free) return free;
  for (let round = 2; ; round++) {
    for (const name of PANE_NAMES) {
      const candidate = `${name} ${round}`;
      if (!used.has(candidate)) return candidate;
    }
  }
}

/** Gives every pane a name, keeping the ones already stored. */
export function nameWorkspacePanes(workspace: Workspace): Workspace {
  const panes = workspace.panes ?? [];
  // The normalised list goes back out even when there was nothing to name: a
  // stored workspace without `panes` would otherwise stay undefined and take
  // down everything that counts its panes.
  if (panes.every((pane) => pane.name)) return { ...workspace, panes };

  const taken = new Set(panes.map((pane) => pane.name).filter(Boolean));
  return {
    ...workspace,
    panes: panes.map((pane) => {
      if (pane.name) return pane;
      const name = pickPaneName(taken);
      taken.add(name);
      return { ...pane, name };
    }),
  };
}

export const paneNames = (panes: Pane[]) => panes.map((pane) => pane.name);
