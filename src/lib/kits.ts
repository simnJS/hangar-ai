import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { TranslateKey, TranslateParams } from "../i18n";
import type { Settings, Workspace, WorkspaceKit } from "../types";

/**
 * Kits (src-tauri/src/kits.rs; the whole contract is docs/kits.md).
 *
 * A kit is a folder with a `hangar-kit.json`, linked from disk or cloned
 * from a GitHub link, that brings a domain into Hangar: a Claude Code
 * plugin (skills, commands, agents, MCP servers) every Claude Code pane of
 * a workspace it is enabled on loads, tabs of its own, and setup commands
 * run once per machine. The workspace holds which kits it has
 * (`Workspace.kits`); the Rust side holds what they are.
 */

/**
 * What a kit's tab may ask Hangar for, by name. Closed: beside these, only
 * `loopback:<port>` (`kitLoopbackPort`) and `command:<name>` (`kitCommandName`),
 * and any other name is a manifest error.
 */
export const KIT_PERMISSIONS = [
  "workspace.read",
  "panes.read",
  "panes.send",
  "files.read",
  "files.write",
  "data.read",
  "data.write",
  "preview.show",
  "notify",
] as const;

export type KitNamedPermission = (typeof KIT_PERMISSIONS)[number];

/** This tab may talk to the local server on that port, through Hangar. */
export type KitLoopbackPermission = `loopback:${number}`;

/** This tab may run that command its kit declares (`command.run`). */
export type KitCommandPermission = `command:${string}`;

export type KitPermission = KitNamedPermission | KitLoopbackPermission | KitCommandPermission;

/**
 * What a workspace stores to say it accepted what a kit's plugin runs by itself:
 * `hooks:` and the first 16 hex of the hash of its hooks and of everything else
 * it starts (kits/plugin.rs). A plugin with only hooks keeps the hash of 1.8.0.
 * Never a tab's: a manifest cannot ask for one, and no method answers to it.
 */
export type KitHooksGrant = `hooks:${string}`;

/**
 * The same for the commands a kit's tabs may run, for this OS (kits/commands.rs):
 * without the current one, a GitHub kit's tabs run none of them.
 */
export type KitCommandsGrant = `commands:${string}`;

/** What a workspace's `granted` may hold. */
export type KitGrant = KitPermission | KitHooksGrant | KitCommandsGrant;

/** Below this, a port belongs to the system. */
export const KIT_LOOPBACK_MIN_PORT = 1024;

/** Whether `port` is one a tab may name: a whole number from 1024 to 65535. */
export const isKitLoopbackPort = (port: unknown): port is number =>
  typeof port === "number" && Number.isInteger(port) && port >= KIT_LOOPBACK_MIN_PORT && port <= 65535;

/**
 * The port of a `loopback:<port>` permission, written plainly (no sign, no
 * leading zero); null for anything else. `loopback_port` in kits/manifest.rs
 * holds the same rule.
 */
export function kitLoopbackPort(name: unknown): number | null {
  if (typeof name !== "string") return null;
  const digits = /^loopback:([1-9][0-9]*)$/.exec(name)?.[1];
  if (!digits) return null;
  const port = Number(digits);
  return isKitLoopbackPort(port) ? port : null;
}

/** How a declared command is named. `valid_command_name` in kits/manifest.rs holds the same rule. */
export const isKitCommandName = (name: unknown): name is string =>
  typeof name === "string" && /^[a-z][a-z0-9-]*$/.test(name);

/** The name in a `command:<name>` permission; null for anything else. */
export function kitCommandName(permission: unknown): string | null {
  if (typeof permission !== "string" || !permission.startsWith("command:")) return null;
  const name = permission.slice("command:".length);
  return isKitCommandName(name) ? name : null;
}

export const isKitPermission = (name: unknown): name is KitPermission =>
  (typeof name === "string" && (KIT_PERMISSIONS as readonly string[]).includes(name)) ||
  kitLoopbackPort(name) !== null ||
  kitCommandName(name) !== null;

/**
 * Whether a tab holding `permissions` reads hidden files, `.env` and its secrets
 * included: it asked for both `files.read` and `files.write` on the same tab, which
 * is what the person sees and accepts at activation. `files.read` alone never does,
 * so no grant given before `files.write` existed reaches further than it did.
 */
export const kitReadsHiddenFiles = (permissions: readonly string[]): boolean =>
  permissions.includes("files.read") && permissions.includes("files.write");

/** The placeholder of a declared command that Hangar fills in with the kit's folder. */
export const KIT_DIR_PLACEHOLDER = "{kitDir}";

/** Whether an argv runs something of the kit's own, which the person is shown by name only. */
export const kitArgvUsesKitDir = (argv: readonly string[]): boolean =>
  argv.slice(1).some((item) => item.includes(KIT_DIR_PLACEHOLDER));

export const isKitHooksGrant = (name: unknown): name is KitHooksGrant =>
  typeof name === "string" && /^hooks:[0-9a-f]{16}$/.test(name);

export const isKitCommandsGrant = (name: unknown): name is KitCommandsGrant =>
  typeof name === "string" && /^commands:[0-9a-f]{16}$/.test(name);

/** The permissions among `names`, once each: the closed set's order, then the ports, then the commands, in order. */
export function kitPermissionsIn(names: Iterable<string>): KitPermission[] {
  const known = new Set(names);
  const ports = [...known]
    .map(kitLoopbackPort)
    .filter((port): port is number => port !== null)
    .sort((a, b) => a - b);
  const commands = [...known]
    .map(kitCommandName)
    .filter((name): name is string => name !== null)
    .sort();
  return [
    ...KIT_PERMISSIONS.filter((permission) => known.has(permission)),
    ...ports.map((port): KitLoopbackPermission => `loopback:${port}`),
    ...commands.map((name): KitCommandPermission => `command:${name}`),
  ];
}

export interface KitTab {
  id: string;
  title: string;
  /** Relative to the kit's root, normalized: `tabs/explorer/index.html`. */
  entry: string;
  icon?: string;
  permissions: KitPermission[];
}

/** One shell command per OS, exactly as the manifest writes it. */
export interface KitSetupRun {
  windows?: string;
  macos?: string;
  linux?: string;
}

export interface KitSetupStep {
  id: string;
  title: string;
  description?: string;
  run: KitSetupRun;
}

/** What a placeholder of a declared command may hold. */
export interface KitCommandArg {
  /** The rule, as the manifest writes it. A value has to match it whole. */
  pattern: string;
  description?: string;
}

/** A command a kit's tabs may run, as `kit_inspect` lists it for this OS. */
export interface KitDeclaredCommand {
  name: string;
  description: string;
  /** The program, then the literal items and the `{placeholders}`, verbatim. */
  argv: string[];
  /** The rule of each placeholder of that argv. */
  args: Record<string, KitCommandArg>;
  timeoutSeconds: number;
}

/** `hangar-kit.json`, as Rust read and checked it: paths are normalized. */
export interface KitManifest {
  schema: 1;
  id: string;
  name: string;
  version: string;
  description: string;
  author?: string;
  homepage?: string;
  icon?: string;
  /** The Hangar versions it works with: `>=1.6.0`. */
  hangar?: string;
  /** The folder of its Claude Code plugin, `.` for the root; absent: none. */
  plugin?: string;
  tabs: KitTab[];
  setup: KitSetupStep[];
}

export type KitSource =
  | { kind: "folder"; path: string }
  | { kind: "git"; url: string; ref?: string; subdir?: string };

export interface KitUpdate {
  current: string;
  /** The version the manifest declares at the remote ref. */
  latest: string;
  available: boolean;
  checkedAt: number;
}

/** An installed kit, as `kits_list` returns it. */
export interface KitInfo {
  id: string;
  source: KitSource;
  /** The kit's root on disk. */
  dir: string;
  installedVersion: string;
  addedAt: number;
  /** Null when the manifest cannot be read or is invalid. */
  manifest: KitManifest | null;
  /**
   * Why the kit cannot be used now; null when it can. A manifest can come
   * with an error: a kit that needs a newer Hangar.
   */
  error: string | null;
  /** The last update check while the app runs; null before any. */
  update: KitUpdate | null;
  /** `hooks:<hash>` of what its plugin runs by itself, hooks and all else; null when it runs nothing. */
  hooksPermission?: string | null;
  /**
   * Whether a workspace has to have accepted all that: every GitHub kit added
   * since that is asked, and an older one once an update changed it.
   */
  hooksConsent?: boolean;
  /** `commands:<hash>` of what its tabs may run on this OS; null without any. */
  commandsPermission?: string | null;
}

/** What a plugin runs by itself, exactly as it writes it. */
export interface KitHook {
  event: string;
  /** Empty: each time the event happens. */
  matcher: string;
  command: string;
}

/** The kinds of `KitRunning`, as `RunKind` in kits/plugin.rs names them. */
export type KitRunningKind =
  | "mcp"
  | "monitor"
  | "lsp"
  | "skillHook"
  | "settings"
  | "grammar"
  | "bin"
  | "binary"
  | "dependency"
  | "unknown";

/**
 * Something a plugin starts by itself beside its hooks, or may, exactly as it
 * writes it. Env values and headers never reach here: only their names.
 */
export interface KitRunning {
  kind: KitRunningKind;
  name: string;
  /** The program, then its arguments. */
  argv?: string[];
  /** A shell command, an address, a path: what `argv` is not. */
  text?: string;
  /** When it says so itself: a monitor's `when`, a language server's file types. */
  when?: string;
  description?: string;
  /** The names of the variables it sets. */
  env?: string[];
  /** The names of the headers it sends. */
  headers?: string[];
  /** What it runs is out of Hangar's reach: a bundle, a broken file, remote code, another plugin. */
  unreadable?: boolean;
}

/** What a kit brings, as `kit_inspect` finds it. */
export interface KitInspection {
  plugin?: { name: string; dir: string };
  skills: string[];
  commands: string[];
  agents: string[];
  /** As the plugin's `.mcp.json` names them, without a prefix. */
  mcpServers: string[];
  /** What its plugin runs by itself, from `hooks/hooks.json` and `plugin.json`. */
  hooks?: KitHook[];
  /** What else it starts by itself, or may. */
  running?: KitRunning[];
  /** `hooks:<hash>` of those very lists: what accepting them stores. */
  hooksPermission?: string | null;
  tabs: KitTab[];
  /** The steps that have a command for this OS. */
  setup: { id: string; title: string; description?: string; command: string }[];
  /** What its tabs may run, for this OS. `commands` above is its plugin's slash commands. */
  declaredCommands?: KitDeclaredCommand[];
  /** `commands:<hash>` of that very list: what accepting it stores. */
  commandsPermission?: string | null;
}

/** A setup step for this machine's OS, and whether it ran with this very command. */
export interface KitSetupStatus {
  id: string;
  title: string;
  description?: string;
  command: string;
  sha256: string;
  done: boolean;
}

export const kitsList = () => invoke<KitInfo[]>("kits_list");

/** Links a folder holding a `hangar-kit.json`, given by its full path. */
export const kitAddFolder = (path: string) => invoke<KitInfo>("kit_add_folder", { path });

/** Clones a kit from an `https://github.com/…` link, `…/tree/<ref>/<folder>` included. */
export const kitAddGit = (url: string) => invoke<KitInfo>("kit_add_git", { url });

/** Deletes only what Hangar cloned, never a linked folder. */
export const kitRemove = (id: string) => invoke<void>("kit_remove", { id });

/** A git kit's remote version against the installed one; null for a folder kit. */
export const kitCheckUpdate = (id: string) => invoke<KitUpdate | null>("kit_check_update", { id });

export const kitUpdate = (id: string) => invoke<KitInfo>("kit_update", { id });

export const kitInspect = (id: string) => invoke<KitInspection>("kit_inspect", { id });

export const kitSetupStatus = (id: string) => invoke<KitSetupStatus[]>("kit_setup_status", { id });

/** Records that a step ran, with the command it has for this OS now. */
export const kitSetupMark = (id: string, stepId: string) =>
  invoke<KitSetupStatus[]>("kit_setup_mark", { id, stepId });

/** Sent, without a payload, when `kitsList` may answer something new. */
export const KITS_CHANGED_EVENT = "kits:changed";

export const onKitsChanged = (fn: () => void): Promise<UnlistenFn> =>
  listen(KITS_CHANGED_EVENT, () => fn());

const KIT_ID = /^[a-z0-9-]{2,40}$/;

/**
 * The kits enabled on a workspace, read defensively: state.json is a file
 * people edit, and an entry that is not `{ id, granted: string[] }` with a
 * kit id is no kit. A permission Hangar does not know is never granted; the
 * pseudo permissions of accepted hooks and commands are kept with the others.
 */
export function workspaceKits(ws: Pick<Workspace, "kits"> | null | undefined): WorkspaceKit[] {
  return readKitEntries(ws?.kits);
}

/** The kits enabled on every workspace (`Settings.kitsEverywhere`), read the same way. */
export function everywhereKits(
  settings: Pick<Settings, "kitsEverywhere"> | null | undefined,
): WorkspaceKit[] {
  return readKitEntries(settings?.kitsEverywhere);
}

/**
 * What decides anything about a workspace's kits: its own entries, then the
 * everywhere ones it does not list itself. A workspace's own entry wins for
 * its grants. `workspaceKits` stays the workspace's own list, which is what
 * the workspace's Kits dialog edits.
 */
export function effectiveKits(
  ws: Pick<Workspace, "kits"> | null | undefined,
  settings: Pick<Settings, "kitsEverywhere"> | null | undefined,
): WorkspaceKit[] {
  const own = workspaceKits(ws);
  const listed = new Set(own.map((kit) => kit.id));
  return [...own, ...everywhereKits(settings).filter((kit) => !listed.has(kit.id))];
}

/**
 * The workspace with its effective kits as `kits`, for the readers that take a
 * workspace (`workspaceKitTabs`, `kitTabState`, …).
 */
export function withEffectiveKits<W extends Pick<Workspace, "kits">>(
  ws: W | null | undefined,
  settings: Pick<Settings, "kitsEverywhere"> | null | undefined,
): Omit<W, "kits"> & { kits: WorkspaceKit[] } {
  return { ...(ws ?? ({} as W)), kits: effectiveKits(ws, settings) };
}

function readKitEntries(kits: unknown): WorkspaceKit[] {
  if (!Array.isArray(kits)) return [];
  const seen = new Set<string>();
  const out: WorkspaceKit[] = [];
  for (const kit of kits) {
    if (!kit || typeof kit !== "object") continue;
    const { id, granted } = kit as Record<string, unknown>;
    if (typeof id !== "string" || !KIT_ID.test(id) || seen.has(id)) continue;
    if (!Array.isArray(granted)) continue;
    seen.add(id);
    const kept = granted.filter(
      (name) => isKitPermission(name) || isKitHooksGrant(name) || isKitCommandsGrant(name),
    );
    out.push({ id, granted: [...new Set<string>(kept)] });
  }
  return out;
}

/** The kits enabled on a workspace, by id. */
export const workspaceKitIds = (
  ws: Pick<Workspace, "kits"> | null | undefined,
  settings?: Pick<Settings, "kitsEverywhere"> | null,
): string[] => effectiveKits(ws, settings).map((kit) => kit.id);

/**
 * What a workspace has to hold for what a kit runs by itself to reach its panes;
 * null when nothing has to be held. Only a GitHub kit is asked: a linked folder is
 * the person's own, read live, and every edit would ask again. A kit that runs
 * nothing has nothing to accept, and one installed before this was asked keeps
 * working as it did until an update changes what it runs.
 */
export function kitHooksGrant(
  kit: Pick<KitInfo, "source" | "hooksPermission" | "hooksConsent">,
): KitHooksGrant | null {
  if (kit.source.kind !== "git" || !kit.hooksConsent) return null;
  return isKitHooksGrant(kit.hooksPermission) ? kit.hooksPermission : null;
}

/**
 * What a workspace has to hold for a kit's tabs to run its commands; null when
 * nothing has to be held. As for hooks, only a GitHub kit is asked: a linked
 * folder is the person's own, read live. No kit predates this, as commands are
 * new, so every GitHub kit that has some is asked.
 */
export function kitCommandsGrant(
  kit: Pick<KitInfo, "source" | "commandsPermission">,
): KitCommandsGrant | null {
  if (kit.source.kind !== "git") return null;
  return isKitCommandsGrant(kit.commandsPermission) ? kit.commandsPermission : null;
}

export interface PaneKits {
  /** What the pane hands `ptySpawn`, and `mcp_scope_write` in `custom` mode. */
  ids: string[];
  /** Enabled, and held out: what they would run by themselves was not accepted here. */
  held: string[];
}

/**
 * The kits a pane of the workspace starts with, `installed` being the kits as
 * they are now. Whatever else is wrong with a kit (not installed, broken) Rust
 * says when the pane starts, so those are passed on.
 */
export function paneKits(
  ws: Pick<Workspace, "kits"> | null | undefined,
  installed: readonly KitInfo[] | null,
  settings?: Pick<Settings, "kitsEverywhere"> | null,
): PaneKits {
  const out: PaneKits = { ids: [], held: [] };
  for (const { id, granted } of effectiveKits(ws, settings)) {
    const kit = installed?.find((entry) => entry.id === id);
    const grant = kit ? kitHooksGrant(kit) : null;
    (grant && !granted.includes(grant) ? out.held : out.ids).push(id);
  }
  return out;
}

/**
 * `paneKits` from the kits on disk right now. A workspace without kits reads
 * nothing. When they cannot be read at all, the ids go on: Rust reads the same
 * index for `pty_spawn` and keeps every kit out, saying why.
 */
export async function loadPaneKits(
  ws: Pick<Workspace, "kits"> | null | undefined,
  settings?: Pick<Settings, "kitsEverywhere"> | null,
): Promise<PaneKits> {
  if (effectiveKits(ws, settings).length === 0) return { ids: [], held: [] };
  const installed: unknown = await kitsList().catch(() => null);
  return paneKits(ws, Array.isArray(installed) ? (installed as KitInfo[]) : null, settings);
}

/** Why a kit stays out of a pane: `pty_spawn`'s `kitIssues`, and `hooks` from `paneKits`. */
export interface KitLaunchIssue {
  id: string;
  reason: "missing" | "broken" | "forbidden" | "hooks";
  /** In English, from Rust, for `broken`. */
  detail?: string;
}

/**
 * The lines a Claude Code pane prints before its agent starts, one per kit
 * left out, and one for every kit a sideloading policy kept out.
 */
export function kitIssueNotes(
  issues: KitLaunchIssue[],
  t: (key: TranslateKey, params?: TranslateParams) => string,
): string[] {
  const notes: string[] = [];
  const forbidden: string[] = [];
  for (const issue of issues) {
    if (issue.reason === "forbidden") forbidden.push(issue.id);
    else if (issue.reason === "missing") notes.push(t("kits.warnMissing", { id: issue.id }));
    else if (issue.reason === "hooks") notes.push(t("kitsManage.warnHooks", { id: issue.id }));
    else notes.push(t("kits.warnBroken", { id: issue.id, detail: issue.detail ?? "" }));
  }
  if (forbidden.length) notes.push(t("kits.warnForbidden", { ids: forbidden.join(", ") }));
  return notes;
}

/*
 * The tab message protocol (docs/kits.md). A kit's tab is an isolated page
 * that talks to Hangar by postMessage; the frame that hosts it answers.
 */

/** How `KIT_METHODS` writes the permission of a call that names its port. */
export const KIT_LOOPBACK_PATTERN = "loopback:<port>";

/** How `KIT_METHODS` writes the permission of a call that names its command. */
export const KIT_COMMAND_PATTERN = "command:<name>";

/** The method a tab calls, and the permission it needs. */
export const KIT_METHODS = {
  "workspace.info": "workspace.read",
  "panes.list": "panes.read",
  "panes.send": "panes.send",
  "files.read": "files.read",
  "files.readBinary": "files.read",
  "files.list": "files.read",
  "files.write": "files.write",
  "data.read": "data.read",
  "data.write": "data.write",
  "loopback.fetch": KIT_LOOPBACK_PATTERN,
  "command.run": KIT_COMMAND_PATTERN,
  "preview.show": "preview.show",
  notify: "notify",
} as const satisfies Record<string, KitNamedPermission | typeof KIT_LOOPBACK_PATTERN | typeof KIT_COMMAND_PATTERN>;

export type KitMethod = keyof typeof KIT_METHODS;

/** The events a tab can listen to, and the permission each needs. */
export const KIT_EVENTS = {
  "panes.changed": "panes.read",
} as const satisfies Record<string, KitNamedPermission>;

export type KitEventName = keyof typeof KIT_EVENTS;

/** Served by the `hangar-kit` scheme to every tab: `window.hangar`. */
export const KIT_SDK_PATH = "/_hangar/kit.js";

/** The most `files.read` returns, in bytes. */
export const KIT_FILE_MAX_BYTES = 1024 * 1024;

/** The most `files.readBinary` reads, in bytes (kits/files.rs). */
export const KIT_BINARY_MAX_BYTES = 10 * 1024 * 1024;

/** The most files `files.list` gives (kits/files.rs). */
export const KIT_LIST_MAX_FILES = 2000;

/** The longest glob `files.list` takes (kits/files.rs). */
export const KIT_LIST_PATTERN_MAX_CHARS = 256;

/** The most `data.read` returns and `data.write` takes, in bytes. */
export const KIT_DATA_MAX_BYTES = 1024 * 1024;

/** The most a `loopback.fetch` answer may hold, in bytes (kits/loopback.rs). */
export const KIT_LOOPBACK_MAX_RESPONSE_BYTES = 2 * 1024 * 1024;
/** The most a `loopback.fetch` may send, in bytes (kits/loopback.rs). */
export const KIT_LOOPBACK_MAX_BODY_BYTES = 2 * 1024 * 1024;
/** How long a `loopback.fetch` waits unless it says otherwise. */
export const KIT_LOOPBACK_TIMEOUT_MS = 5_000;
export const KIT_LOOPBACK_MAX_TIMEOUT_MS = 15_000;

/** The error code of a call the tab has no permission for. */
export const KIT_DENIED = "denied";

export interface KitRequest {
  hangar: 1;
  id: string | number;
  method: string;
  params?: unknown;
}

export type KitResponse =
  | { hangar: 1; id: string | number; result: unknown }
  | { hangar: 1; id: string | number; error: { code: string; message: string } };

export interface KitEventMessage {
  hangar: 1;
  event: string;
  data: unknown;
}

/**
 * Whether a tab granted `granted` may call `method`; unknown methods never.
 * `loopback.fetch` needs the permission of the very port its `params` name, and
 * `command.run` that of the very command.
 */
export function kitMayCall(method: string, granted: readonly string[], params?: unknown): boolean {
  if (!Object.prototype.hasOwnProperty.call(KIT_METHODS, method)) return false;
  const permission = KIT_METHODS[method as KitMethod];
  if (permission === KIT_LOOPBACK_PATTERN) {
    const port = (params as { port?: unknown } | null | undefined)?.port;
    return isKitLoopbackPort(port) && granted.includes(`loopback:${port}`);
  }
  if (permission === KIT_COMMAND_PATTERN) {
    const name = (params as { name?: unknown } | null | undefined)?.name;
    return isKitCommandName(name) && granted.includes(`command:${name}`);
  }
  return granted.includes(permission);
}
