import { leafIds, normalizeTree } from "../lib/layout";
import { AGENTS, type Pane, type Workspace, type WorkspaceFolder } from "../types";

/**
 * What the navigator lists, worked out from the store and the filter without
 * touching React, so the rules below can be tested on their own.
 *
 * Rows are addressed by a key that says what they are — `folder:<id>`,
 * `ws:<id>`, `pane:<id>` — because the keyboard walks folders, workspaces and
 * agents as one list, and ids alone could not tell which kind a row is.
 */

export interface NavWorkspace {
  workspace: Workspace;
  /** The agent rows under it, in reading order: every pane, or the ones the filter kept. */
  panes: Pane[];
  expanded: boolean;
  /** Its chevron does something: the active workspace and one held open by a filter stay open. */
  foldable: boolean;
}

export interface NavFolder {
  folder: WorkspaceFolder;
  workspaces: NavWorkspace[];
  expanded: boolean;
  /** Workspaces filed in it, whatever the filter lets through. */
  size: number;
}

export type NavRowKind = "folder" | "ws" | "pane";

/** One row the keyboard can land on, in screen order. */
export interface NavRow {
  key: string;
  kind: NavRowKind;
  id: string;
  /** The row it sits under, for Left; null at the top level. */
  parent: string | null;
  /** Folders and workspaces: whether their children are on screen. */
  expanded?: boolean;
  foldable?: boolean;
}

export interface NavTree {
  folders: NavFolder[];
  loose: NavWorkspace[];
  rows: NavRow[];
  /** A filter is narrowing the list, so an empty one means "nothing matches". */
  filtering: boolean;
}

export interface NavInput {
  workspaces: Workspace[];
  folders: WorkspaceFolder[];
  activeId: string | null;
  /** Workspaces opened by hand, by id. The active one is open regardless. */
  opened: Record<string, boolean>;
  /** The filter field, as typed. */
  query: string;
  /** Keep only the agents that wait for an answer or are done and waiting for you. */
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

export function buildNavTree(input: NavInput): NavTree {
  const { workspaces, folders, activeId, opened, attentionOnly, wantsYou } = input;
  const needle = fold(input.query.trim());
  const filtering = needle !== "" || attentionOnly;

  function entry(ws: Workspace, folderHit: boolean): NavWorkspace | null {
    const active = ws.id === activeId;
    const ordered = readingOrder(ws);
    if (!filtering) {
      const expanded = active || Boolean(opened[ws.id]);
      return { workspace: ws, panes: ordered, expanded, foldable: !active };
    }

    // Named after the query (or filed in a folder that is): every agent
    // counts as a match. Otherwise only the agents whose own name, or
    // whose kind of agent, holds it.
    const named = needle === "" || folderHit || fold(ws.name).includes(needle);
    const own = (pane: Pane) =>
      fold(pane.name).includes(needle) || fold(agentLabel(pane)).includes(needle);
    const panes = ordered.filter(
      (pane) => (named || own(pane)) && (!attentionOnly || wantsYou(pane.id)),
    );

    if (attentionOnly) {
      // The point is the agents: a workspace is only there for theirs.
      return panes.length ? { workspace: ws, panes, expanded: true, foldable: false } : null;
    }
    if (!named && !panes.length) return null;
    // Found through one of its agents: open, or the match would be hidden.
    const heldOpen = !named;
    const expanded = heldOpen || active || Boolean(opened[ws.id]);
    return { workspace: ws, panes, expanded, foldable: !active && !heldOpen };
  }

  const known = new Set(folders.map((folder) => folder.id));
  const navFolders: NavFolder[] = [];
  for (const folder of folders) {
    const filed = workspaces.filter((ws) => ws.folderId === folder.id);
    const folderHit = needle !== "" && fold(folder.name).includes(needle);
    const shown = filed.flatMap((ws) => entry(ws, folderHit) ?? []);
    // Under a filter a folder is only a heading for what it lets through,
    // held open so that is visible — unless its own name is the match.
    if (filtering && !shown.length && !(folderHit && !attentionOnly)) continue;
    navFolders.push({
      folder,
      workspaces: shown,
      expanded: filtering || !folder.collapsed,
      size: filed.length,
    });
  }

  const loose = workspaces
    .filter((ws) => !ws.folderId || !known.has(ws.folderId))
    .flatMap((ws) => entry(ws, false) ?? []);

  const rows: NavRow[] = [];
  const pushWorkspace = (entry: NavWorkspace, parent: string | null) => {
    const key = rowKey("ws", entry.workspace.id);
    rows.push({
      key,
      kind: "ws",
      id: entry.workspace.id,
      parent,
      expanded: entry.expanded,
      foldable: entry.foldable,
    });
    if (!entry.expanded) return;
    for (const pane of entry.panes) {
      rows.push({ key: rowKey("pane", pane.id), kind: "pane", id: pane.id, parent: key });
    }
  };
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
    if (entry.expanded) entry.workspaces.forEach((ws) => pushWorkspace(ws, key));
  }
  loose.forEach((ws) => pushWorkspace(ws, null));

  return { folders: navFolders, loose, rows, filtering };
}
