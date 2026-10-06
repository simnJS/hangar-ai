import type { Pane, Workspace } from "../types";

/**
 * Panes are called by name rather than numbered.
 *
 * A number is a position, and positions move: split, close or drag a pane and
 * every index after it shifts, so "pane 3" means something else a second
 * later. A name is attached to the pane itself and survives all of it, which
 * is what makes it usable in a sentence — "Leo is done, Ava is still running".
 *
 * A name is unique across every workspace, not only within its own: Claude
 * Code sessions reach each other by name, and two "Ava" running in two
 * workspaces could not be told apart.
 */

/**
 * Short, unambiguous, easy to say out loud. The first sixteen are the ones
 * panes were named with before names became unique across workspaces, kept
 * first and in order so existing panes keep theirs.
 */
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
  "Ada",
  "Ben",
  "Cora",
  "Dex",
  "Emma",
  "Gus",
  "Hugo",
  "Iris",
  "Jack",
  "Kai",
  "Lola",
  "Milo",
  "Otto",
  "Quinn",
  "Rosa",
  "Seth",
  "Tara",
  "Vera",
  "Will",
  "Yara",
  "Zack",
  "Bram",
  "Ezra",
  "Faye",
  "Hana",
  "Juno",
  "Kit",
  "Lars",
  "Omar",
  "Ruby",
  "Tess",
  "Vic",
];

/** First free name; falls back to "Ava 2" once the list runs out. */
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

/** The next `count` free names, in the order panes would be given them. */
export function pickPaneNames(count: number, taken: Iterable<string>): string[] {
  const used = new Set(taken);
  const names: string[] = [];
  for (let i = 0; i < count; i++) {
    const name = pickPaneName(used);
    used.add(name);
    names.push(name);
  }
  return names;
}

/** Every name in use, in every workspace. */
export const allPaneNames = (workspaces: Workspace[]) =>
  workspaces.flatMap((ws) => (ws.panes ?? []).map((pane) => pane.name).filter(Boolean));

/**
 * Gives every pane a name no other pane has, in any workspace. Names already
 * stored are kept where they are the first of their kind — in the order the
 * workspaces are listed — and only the later duplicates, plus the panes that
 * had no name at all, get a new one. Returns the same objects when nothing
 * had to change.
 */
export function uniquePaneNames(workspaces: Workspace[]): Workspace[] {
  const seen = new Set<string>();
  const clashing = new Set<Pane>();
  for (const ws of workspaces) {
    for (const pane of ws.panes ?? []) {
      if (!pane.name || seen.has(pane.name)) clashing.add(pane);
      else seen.add(pane.name);
    }
  }
  if (clashing.size === 0) return workspaces;

  return workspaces.map((ws) => {
    const panes = ws.panes ?? [];
    if (!panes.some((pane) => clashing.has(pane))) return { ...ws, panes };
    return {
      ...ws,
      panes: panes.map((pane) => {
        if (!clashing.has(pane)) return pane;
        const name = pickPaneName(seen);
        seen.add(name);
        return { ...pane, name };
      }),
    };
  });
}

export const paneNames = (panes: Pane[]) => panes.map((pane) => pane.name);

/**
 * The name a Claude Code session runs under — what other sessions list and
 * message it by: the pane's name, then its workspace's. The workspace is there
 * for the agents, so one can tell which project the agent it is about to write
 * to works on; the interface keeps showing the pane's name alone.
 *
 * Characters a shell would read as quoting or expansion are left out of the
 * workspace part, so the name can always go on the command line.
 */
export function sessionName(paneName: string, workspaceName: string): string {
  const workspace = workspaceName
    .replace(/["'`$\\%!^&|<>]/g, "")
    .replace(/\s+/g, " ")
    .trim();
  return workspace ? `${paneName} (${workspace})` : paneName;
}
