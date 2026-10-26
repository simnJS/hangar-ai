import { leafIds, normalizeTree } from "../lib/layout";
import { AGENTS, type Pane, type Workspace, type WorkspaceFolder } from "../types";

/**
 * What the navigator lists, worked out from the store and the filter without
 * touching React, so the rules below can be tested on their own.
 *
 * Rows are addressed by a key that says what they are — `folder:<id>`,
 * `ws:<id>` — because the keyboard walks folders and workspaces as one list,
 * and ids alone could not tell which kind a row is.
 *
 * The workspaces filed in no folder come first, then each folder: a folder is
 * a heading over the workspaces under it, so whatever follows its last one
 * would read as filed in it too.
 */

export interface NavFolder {
  folder: WorkspaceFolder;
  workspaces: Workspace[];
  expanded: boolean;
  /** Workspaces filed in it, whatever the filter lets through. */
  size: number;
}

export type NavRowKind = "folder" | "ws";

/** One row the keyboard can land on, in screen order. */
export interface NavRow {
  key: string;
  kind: NavRowKind;
  id: string;
  /** The row it sits under, for Left; null at the top level. */
  parent: string | null;
  /** Folders: whether their workspaces are on screen. */
  expanded?: boolean;
  foldable?: boolean;
}

export interface NavTree {
  loose: Workspace[];
  folders: NavFolder[];
  rows: NavRow[];
  /** A filter is narrowing the list, so an empty one means "nothing matches". */
  filtering: boolean;
}

export interface NavInput {
  workspaces: Workspace[];
  folders: WorkspaceFolder[];
  /** The filter field, as typed. */
  query: string;
  /** Keep only the workspaces with an agent waiting for an answer or done and waiting for you. */
  attentionOnly: boolean;
  /** Whether a pane wants you now; only asked when `attentionOnly` is on. */
  wantsYou: (paneId: string) => boolean;
}

export const rowKey = (kind: NavRowKind, id: string) => `${kind}:${id}`;

/** Case and accents aside, so "cleo" finds "Cléo" and "ecran" finds "écran". */
export function fold(text: string): string {
  return text.normalize("NFD").replace(/\p{M}/gu, "").toLowerCase();
}

const agentLabel = (pane: Pane) => AGENTS.find((a) => a.id === pane.agent)?.label ?? pane.agent;

/**
 * A workspace's panes as the grid shows them, left to right then top to
 * bottom, which is the order people read them in. The split tree only lists
 * ids; an empty workspace would otherwise come back as one nameless leaf.
 */
export function readingOrder(ws: Workspace): Pane[] {
  if (!ws.panes.length) return [];
  const byId = new Map(ws.panes.map((pane) => [pane.id, pane]));
  return leafIds(normalizeTree(ws.panes, ws.tree)).flatMap((id) => {
    const pane = byId.get(id);
    return pane ? [pane] : [];
  });
}

const segments = (path: string) => path.split(/[\\/]+/).filter(Boolean);

/**
 * What tells apart workspaces that share a name, since their folder is no
 * longer written under it: for each, the last folder of its path that none
 * of its namesakes has at the same depth — "api" in work and in perso reads
 * "api · work" and "api · perso" — and no more of the path than that. A
 * folder that only repeats the name is passed over; namesakes opened on the
 * same folder are numbered instead. A name no other workspace has gets
 * nothing, which is almost always.
 */
export function namesakeHints(workspaces: Workspace[]): Map<string, string> {
  const hints = new Map<string, string>();
  const byName = new Map<string, Workspace[]>();
  for (const ws of workspaces) {
    const key = fold(ws.name.trim());
    byName.set(key, [...(byName.get(key) ?? []), ws]);
  }
  for (const [name, group] of byName) {
    if (group.length < 2) continue;
    const paths = group.map((ws) => segments(ws.cwd));
    const depth = Math.max(...paths.map((path) => path.length));
    const unhinted: Workspace[] = [];
    group.forEach((ws, index) => {
      const own = paths[index];
      for (let k = 1; k <= own.length && k <= depth; k++) {
        const segment = own[own.length - k];
        if (fold(segment) === name) continue;
        const shared = paths.some(
          (other, at) => at !== index && fold(other[other.length - k] ?? "") === fold(segment),
        );
        if (!shared) {
          hints.set(ws.id, segment);
          return;
        }
      }
      unhinted.push(ws);
    });
    // One left without a word is told apart by the others' already.
    if (unhinted.length > 1) unhinted.forEach((ws, index) => hints.set(ws.id, String(index + 1)));
  }
  return hints;
}

export function buildNavTree(input: NavInput): NavTree {
  const { workspaces, folders, attentionOnly, wantsYou } = input;
  const needle = fold(input.query.trim());
  const filtering = needle !== "" || attentionOnly;

  /**
   * Found by its own name, its folder's, or one of its agents' — their names
   * are not on screen any more, but "eve" still finds the workspace Eve is in.
   */
  function shown(ws: Workspace, folderHit: boolean): boolean {
    if (attentionOnly && !ws.panes.some((pane) => wantsYou(pane.id))) return false;
    if (needle === "" || folderHit || fold(ws.name).includes(needle)) return true;
    return ws.panes.some(
      (pane) => fold(pane.name).includes(needle) || fold(agentLabel(pane)).includes(needle),
    );
  }

  const known = new Set(folders.map((folder) => folder.id));
  const loose = workspaces.filter(
    (ws) => (!ws.folderId || !known.has(ws.folderId)) && shown(ws, false),
  );

  const navFolders: NavFolder[] = [];
  for (const folder of folders) {
    const filed = workspaces.filter((ws) => ws.folderId === folder.id);
    const folderHit = needle !== "" && fold(folder.name).includes(needle);
    const kept = filed.filter((ws) => shown(ws, folderHit));
    // Under a filter a folder is only a heading for what it lets through,
    // held open so that is visible — unless its own name is the match.
    if (filtering && !kept.length && !(folderHit && !attentionOnly)) continue;
    navFolders.push({
      folder,
      workspaces: kept,
      expanded: filtering || !folder.collapsed,
      size: filed.length,
    });
  }

  const rows: NavRow[] = loose.map((ws) => ({
    key: rowKey("ws", ws.id),
    kind: "ws",
    id: ws.id,
    parent: null,
  }));
  for (const entry of navFolders) {
    const key = rowKey("folder", entry.folder.id);
    rows.push({
      key,
      kind: "folder",
      id: entry.folder.id,
      parent: null,
      expanded: entry.expanded,
      foldable: !filtering,
    });
    if (!entry.expanded) continue;
    for (const ws of entry.workspaces) {
      rows.push({ key: rowKey("ws", ws.id), kind: "ws", id: ws.id, parent: key });
    }
  }

  return { loose, folders: navFolders, rows, filtering };
}
