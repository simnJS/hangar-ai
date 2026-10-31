import type { TranslateKey, Translator } from "../i18n";
import type { Pane, Workspace, WorkspaceKit } from "../types";
import {
  isKitCommandsGrant,
  isKitHooksGrant,
  kitCommandName,
  kitCommandsGrant,
  kitHooksGrant,
  kitLoopbackPort,
  kitPermissionsIn,
  workspaceKits,
  type KitGrant,
  type KitInfo,
  type KitInspection,
  type KitNamedPermission,
  type KitPermission,
  type KitRunning,
  type KitRunningKind,
  type KitTab,
} from "./kits";

/**
 * What enabling a kit on a workspace means, as pure functions: which
 * permissions its tabs ask for, which of them the workspace has already
 * accepted, what a toggle writes into `Workspace.kits`, and which panes need a
 * restart to see a kit. The screens (components/KitActivation) only draw it.
 *
 * What a GitHub kit's plugin runs by itself goes through the same diff, as the
 * pseudo permission `hooks:<hash>` (lib/kits › kitHooksGrant): its hooks and all
 * else it starts, changed, are something new to accept, and the kit stays out of
 * the workspace's new panes until they are.
 * Its declared commands do too, as `commands:<hash>` (lib/kits › kitCommandsGrant):
 * changed, its tabs run none of them until they are accepted again.
 */

/** Every permission has its sentence in the dictionary, in plain words. */
export const PERMISSION_LABEL: Record<KitNamedPermission, TranslateKey> = {
  "workspace.read": "kitsManage.permWorkspaceRead",
  "panes.read": "kitsManage.permPanesRead",
  "panes.send": "kitsManage.permPanesSend",
  "files.read": "kitsManage.permFilesRead",
  "files.write": "kitsTab.permFilesWrite",
  "data.read": "kitsManage.permDataRead",
  "data.write": "kitsManage.permDataWrite",
  "preview.show": "kitsManage.permPreviewShow",
  notify: "kitsManage.permNotify",
};

/** A permission in plain words, a local server's by its port, a command's by its name. */
export function permissionLabel(t: Translator, permission: KitPermission): string {
  const command = kitCommandName(permission);
  if (command !== null) return t("kitsTab.permCommand", { name: command });
  const port = kitLoopbackPort(permission);
  // As text: `t` writes a number the way the language does, and a port has no
  // thousands.
  return port === null
    ? t(PERMISSION_LABEL[permission as KitNamedPermission])
    : t("kitsManage.permLoopback", { port: String(port) });
}

/** What the tabs of a kit ask for altogether, once each, in the closed set's order, then the ports, then the commands. */
export function requestedPermissions(tabs: readonly Pick<KitTab, "permissions">[]): KitPermission[] {
  return kitPermissionsIn(tabs.flatMap((tab) => tab.permissions));
}

export interface PermissionDiff {
  /** Asked for and already accepted. */
  kept: KitGrant[];
  /** Asked for and not accepted: a new kit, or an update that asks for more or changes its hooks. */
  added: KitGrant[];
}

/**
 * Splits what a kit asks for by what the workspace accepted. A permission the
 * kit no longer asks for is simply dropped: nothing is lost by it.
 */
export function permissionDiff(
  requested: readonly KitGrant[],
  granted: readonly string[],
): PermissionDiff {
  const kept: KitGrant[] = [];
  const added: KitGrant[] = [];
  for (const permission of requested) (granted.includes(permission) ? kept : added).push(permission);
  return { kept, added };
}

/** Whether what a kit asks for waits for its hooks to be accepted on this workspace. */
export const hooksToAccept = (diff: Pick<PermissionDiff, "added">): boolean =>
  diff.added.some(isKitHooksGrant);

/** Whether what a kit asks for waits for its commands to be accepted on this workspace. */
export const commandsToAccept = (diff: Pick<PermissionDiff, "added">): boolean =>
  diff.added.some(isKitCommandsGrant);

/** Whether a kit's commands are all it waits for: its plugin still reaches the panes then. */
export const onlyCommandsToAccept = (diff: Pick<PermissionDiff, "added">): boolean =>
  diff.added.length > 0 && diff.added.every(isKitCommandsGrant);

/** Characters that do not show, or turn the text around them: a line could read as another. */
const UNSEEN = /[\p{Cc}\p{Cf}]/gu;

const withUnseenWritten = (text: string) =>
  text.replace(UNSEEN, (char) => `\\u{${char.codePointAt(0)?.toString(16)}}`);

/**
 * An argv on one line, as it is. An item holding a space, a quote or nothing at
 * all is quoted, so `["gh", "run view"]` does not read as `["gh", "run", "view"]`,
 * and what would not show is written out.
 */
export function kitArgvLine(argv: readonly string[]): string {
  return argv
    .map((item) =>
      item === "" || /[\s"']/.test(item) || item.search(UNSEEN) !== -1
        ? withUnseenWritten(JSON.stringify(item))
        : item,
    )
    .join(" ");
}

/** A rule as the person is shown it: as written, with what would not show written out. */
export const kitRuleText = (pattern: string): string => withUnseenWritten(pattern);

/** What the activation view groups: `kit_inspect`'s kinds, and a hooks file's `modules`. */
export type KitRunningGroupKind = KitRunningKind | "module";

/** The order of the groups: what starts on its own first, what Hangar cannot make out last. */
const RUNNING_GROUP_ORDER: readonly KitRunningGroupKind[] = [
  "module",
  "mcp",
  "monitor",
  "lsp",
  "skillHook",
  "settings",
  "grammar",
  "bin",
  "binary",
  "dependency",
  "unknown",
];

/** The title of each group, and its one sentence saying when it runs. */
export const RUNNING_LABEL: Record<KitRunningGroupKind, { title: TranslateKey; when: TranslateKey }> = {
  module: { title: "kitsManage.runModule", when: "kitsManage.runModuleWhen" },
  mcp: { title: "kitsManage.runMcp", when: "kitsManage.runMcpWhen" },
  monitor: { title: "kitsManage.runMonitor", when: "kitsManage.runMonitorWhen" },
  lsp: { title: "kitsManage.runLsp", when: "kitsManage.runLspWhen" },
  skillHook: { title: "kitsManage.runSkillHook", when: "kitsManage.runSkillHookWhen" },
  settings: { title: "kitsManage.runSettings", when: "kitsManage.runSettingsWhen" },
  grammar: { title: "kitsManage.runGrammar", when: "kitsManage.runGrammarWhen" },
  bin: { title: "kitsManage.runBin", when: "kitsManage.runBinWhen" },
  binary: { title: "kitsManage.runBinary", when: "kitsManage.runBinaryWhen" },
  dependency: { title: "kitsManage.runDependency", when: "kitsManage.runDependencyWhen" },
  unknown: { title: "kitsManage.runUnknown", when: "kitsManage.runUnknownWhen" },
};

export interface KitRunningGroup {
  kind: KitRunningGroupKind;
  items: Omit<KitRunning, "kind">[];
}

/** A hooks file's `modules` entry is listed among its hooks, under this event (kits/hooks.rs). */
const MODULES_EVENT = "modules";

/** The hooks proper: a hooks module is code of its own, and has its own group. */
export const kitPlainHooks = (inspection: Pick<KitInspection, "hooks"> | null): NonNullable<KitInspection["hooks"]> =>
  (inspection?.hooks ?? []).filter((hook) => hook.event !== MODULES_EVENT);

/**
 * What a plugin starts by itself beside its hooks, by kind, in the view's order.
 * A kind this build does not know counts as not understood: nothing is left out.
 */
export function kitRunningGroups(inspection: Pick<KitInspection, "hooks" | "running"> | null): KitRunningGroup[] {
  const modules = (inspection?.hooks ?? [])
    .filter((hook) => hook.event === MODULES_EVENT)
    .map((hook) => ({ name: hook.event, text: hook.command }));
  const byKind = new Map<KitRunningGroupKind, KitRunningGroup["items"]>();
  if (modules.length > 0) byKind.set("module", modules);
  for (const item of inspection?.running ?? []) {
    const kind = RUNNING_GROUP_ORDER.includes(item.kind) ? item.kind : "unknown";
    byKind.set(kind, [...(byKind.get(kind) ?? []), item]);
  }
  return RUNNING_GROUP_ORDER.flatMap((kind) => {
    const items = byKind.get(kind);
    return items ? [{ kind, items }] : [];
  });
}

/**
 * What an item starts, for its block: null when all Hangar can give is its name.
 * A line break stays one: what a skill declares takes several lines.
 */
export function kitRunningLine(item: Pick<KitRunning, "argv" | "text">): string | null {
  if (item.argv && item.argv.length > 0) return kitArgvLine(item.argv);
  return item.text ? item.text.split("\n").map(kitRuleText).join("\n") : null;
}

export type KitState =
  /** Not enabled on the workspace. */
  | "off"
  /** Enabled, with every permission its tabs ask for, and its hooks, accepted. */
  | "on"
  /** Enabled, but an update asks for permissions not accepted yet, or changed its hooks. */
  | "review"
  /** Enabled, but the kit cannot be used now (broken, gone). */
  | "broken";

/** Where a kit stands on a workspace; `requested` is null for a kit that cannot be used. */
export function kitState(
  entry: WorkspaceKit | undefined,
  requested: readonly KitGrant[] | null,
): KitState {
  if (!entry) return "off";
  if (requested === null) return "broken";
  return permissionDiff(requested, entry.granted).added.length > 0 ? "review" : "on";
}

/**
 * What a workspace has to have accepted of a kit: the permissions its tabs ask
 * for, from its manifest, and its hooks and commands when they are asked about.
 * Null when the kit cannot be used.
 */
export function kitRequested(
  kit: Pick<KitInfo, "manifest" | "error" | "source" | "hooksPermission" | "hooksConsent" | "commandsPermission">,
): KitGrant[] | null {
  if (!kit.manifest || kit.error) return null;
  const hooks = kitHooksGrant(kit);
  const commands = kitCommandsGrant(kit);
  return [...requestedPermissions(kit.manifest.tabs), ...(hooks ? [hooks] : []), ...(commands ? [commands] : [])];
}

/**
 * What accepting a kit stores, from what the person was shown: its hashes are the
 * inspection's, not the kit's as it may be now, so hooks or commands that changed
 * while the view was open stay to be accepted. A linked folder's store nothing.
 */
export function acceptedGrants(
  kit: Pick<KitInfo, "source">,
  inspection: Pick<KitInspection, "tabs" | "hooksPermission" | "commandsPermission"> | null,
): KitGrant[] {
  const permissions = requestedPermissions(inspection?.tabs ?? []);
  if (kit.source.kind !== "git") return permissions;
  const hooks = inspection?.hooksPermission;
  const commands = inspection?.commandsPermission;
  return [
    ...permissions,
    ...(isKitHooksGrant(hooks) ? [hooks] : []),
    ...(isKitCommandsGrant(commands) ? [commands] : []),
  ];
}

/** The workspace's kits with `id` enabled, accepting exactly `granted`. */
export function enableKit(
  kits: readonly WorkspaceKit[],
  id: string,
  granted: readonly KitGrant[],
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
  /** Permissions the new version asks for that the old one did not, or its changed hooks. */
  newPermissions: KitGrant[];
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
