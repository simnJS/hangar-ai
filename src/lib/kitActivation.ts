import type { TranslateKey } from "../i18n";
import type { Pane, Workspace, WorkspaceKit } from "../types";
import { KIT_PERMISSIONS, workspaceKits, type KitInfo, type KitPermission, type KitTab } from "./kits";

/**
 * What enabling a kit on a workspace means, as pure functions: which
 * permissions its tabs ask for, which of them the workspace has already
 * accepted, what a toggle writes into `Workspace.kits`, and which panes need a
 * restart to see a kit. The screens (components/KitActivation) only draw it.
 */

/** Every permission has its sentence in the dictionary, in plain words. */
export const PERMISSION_LABEL: Record<KitPermission, TranslateKey> = {
  "workspace.read": "kitsManage.permWorkspaceRead",
  "panes.read": "kitsManage.permPanesRead",
  "panes.send": "kitsManage.permPanesSend",
  "files.read": "kitsManage.permFilesRead",
  "preview.show": "kitsManage.permPreviewShow",
  notify: "kitsManage.permNotify",
};

/** What the tabs of a kit ask for altogether, once each, in the closed set's order. */
export function requestedPermissions(tabs: readonly Pick<KitTab, "permissions">[]): KitPermission[] {
  const asked = new Set<string>(tabs.flatMap((tab) => tab.permissions));
  return KIT_PERMISSIONS.filter((permission) => asked.has(permission));
}

export interface PermissionDiff {
  /** Asked for and already accepted. */
  kept: KitPermission[];
  /** Asked for and not accepted: a new kit, or an update that asks for more. */
  added: KitPermission[];
}

/**
 * Splits what a kit asks for by what the workspace accepted. A permission the
 * kit no longer asks for is simply dropped: nothing is lost by it.
 */
export function permissionDiff(
  requested: readonly KitPermission[],
  granted: readonly string[],
): PermissionDiff {
  const kept: KitPermission[] = [];
  const added: KitPermission[] = [];
  for (const permission of requested) (granted.includes(permission) ? kept : added).push(permission);
  return { kept, added };
}

export type KitState =
  /** Not enabled on the workspace. */
  | "off"
  /** Enabled, with every permission its tabs ask for accepted. */
  | "on"
  /** Enabled, but an update asks for permissions not accepted yet. */
  | "review"
  /** Enabled, but the kit cannot be used now (broken, gone). */
  | "broken";

/** Where a kit stands on a workspace; `requested` is null for a kit that cannot be used. */
export function kitState(
  entry: WorkspaceKit | undefined,
  requested: readonly KitPermission[] | null,
): KitState {
  if (!entry) return "off";
  if (requested === null) return "broken";
  return permissionDiff(requested, entry.granted).added.length > 0 ? "review" : "on";
}

/** The permissions a kit's tabs ask for, from its manifest alone; null when it cannot be used. */
export const kitRequested = (kit: Pick<KitInfo, "manifest" | "error">): KitPermission[] | null =>
  kit.manifest && !kit.error ? requestedPermissions(kit.manifest.tabs) : null;

/** The workspace's kits with `id` enabled, accepting exactly `granted`. */
export function enableKit(
  kits: readonly WorkspaceKit[],
  id: string,
  granted: readonly KitPermission[],
): WorkspaceKit[] {
  const entry: WorkspaceKit = { id, granted: [...granted] };
  return kits.some((kit) => kit.id === id)
    ? kits.map((kit) => (kit.id === id ? entry : kit))
    : [...kits, entry];
}

/** The workspace's kits without `id`. */
export const disableKit = (kits: readonly WorkspaceKit[], id: string): WorkspaceKit[] =>
  kits.filter((kit) => kit.id !== id);

/**
 * The panes that carry kits: Claude Code's. Other agents and plain shells never
 * load a kit's plugin, so they have nothing to restart for.
 */
export const kitPanes = (ws: Pick<Workspace, "panes">): Pane[] =>
  (ws.panes ?? []).filter((pane) => pane.agent === "claude");

/**
 * The panes a kit change still needs a restart for: those running now keep the
 * kits they started with. The respawn keeps each pane's `sessionId`, so the
 * conversation resumes.
 */
export const panesToRestart = (ws: Pick<Workspace, "panes">): string[] =>
  kitPanes(ws).map((pane) => pane.id);

/** Whether two lists hold the same kits with the same grants. */
export function sameKits(a: readonly WorkspaceKit[], b: readonly WorkspaceKit[]): boolean {
  if (a.length !== b.length) return false;
  const byId = new Map(b.map((kit) => [kit.id, kit]));
  return a.every((kit) => {
    const other = byId.get(kit.id);
    return !!other && [...kit.granted].sort().join() === [...other.granted].sort().join();
  });
}

/** The workspace's kits through the defensive reader. */
export const kitsOf = (ws: Pick<Workspace, "kits"> | null | undefined): WorkspaceKit[] =>
  workspaceKits(ws);

/** The source of a kit in one line, for a list: the folder's path or the GitHub link. */
export function sourceLine(source: KitInfo["source"]): { kind: "folder" | "git"; text: string } {
  if (source.kind === "folder") return { kind: "folder", text: source.path };
  const where = [source.ref, source.subdir].filter(Boolean).join(" · ");
  return { kind: "git", text: where ? `${source.url} (${where})` : source.url };
}

/** A kit's name for a list: its manifest's, or its id when the manifest cannot be read. */
export const kitName = (kit: Pick<KitInfo, "id" | "manifest">): string =>
  kit.manifest?.name ?? kit.id;

export interface UpdateOutcome {
  from: string;
  to: string;
  /** Permissions the new version asks for that the old one did not. */
  newPermissions: KitPermission[];
}

/** What an update changed, for the message after it. */
export function updateOutcome(before: KitInfo, after: KitInfo): UpdateOutcome {
  const was = kitRequested(before) ?? [];
  const now = kitRequested(after) ?? [];
  return {
    from: before.installedVersion,
    to: after.installedVersion,
    newPermissions: now.filter((permission) => !was.includes(permission)),
  };
}
