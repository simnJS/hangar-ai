import type { Workspace, WorkspaceFolder } from "../types";

/**
 * The workspaces the way the sidebar lists them: each folder's in folder order,
 * then the ones at the top level.
 *
 * Shortcuts that step through workspaces follow this rather than the stored
 * order, so Ctrl+3 is the third row on screen. A collapsed folder still
 * counts: numbering that shifted every time one was folded would be no use.
 */
export function sidebarOrder(
  workspaces: Workspace[],
  folders: WorkspaceFolder[],
): Workspace[] {
  const known = new Set(folders.map((folder) => folder.id));
  return [
    ...folders.flatMap((folder) => workspaces.filter((ws) => ws.folderId === folder.id)),
    ...workspaces.filter((ws) => ws.folderId === null || !known.has(ws.folderId)),
  ];
}
