import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { TranslateKey, TranslateParams } from "../i18n";
import type { Workspace, WorkspaceKit } from "../types";

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

/** What a kit's tab may ask Hangar for. Closed: any other name is a manifest error. */
export const KIT_PERMISSIONS = [
  "workspace.read",
  "panes.read",
  "panes.send",
  "files.read",
  "preview.show",
  "notify",
] as const;

export type KitPermission = (typeof KIT_PERMISSIONS)[number];

export const isKitPermission = (name: unknown): name is KitPermission =>
  typeof name === "string" && (KIT_PERMISSIONS as readonly string[]).includes(name);

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
}

/** What a kit brings, as `kit_inspect` finds it. */
export interface KitInspection {
  plugin?: { name: string; dir: string };
  skills: string[];
  commands: string[];
  agents: string[];
  /** As the plugin's `.mcp.json` names them, without a prefix. */
  mcpServers: string[];
  tabs: KitTab[];
  /** The steps that have a command for this OS. */
  setup: { id: string; title: string; description?: string; command: string }[];
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
 * kit id is no kit. A permission Hangar does not know is never granted.
 */
export function workspaceKits(ws: Pick<Workspace, "kits"> | null | undefined): WorkspaceKit[] {
  const kits: unknown = ws?.kits;
  if (!Array.isArray(kits)) return [];
  const seen = new Set<string>();
  const out: WorkspaceKit[] = [];
  for (const kit of kits) {
    if (!kit || typeof kit !== "object") continue;
    const { id, granted } = kit as Record<string, unknown>;
    if (typeof id !== "string" || !KIT_ID.test(id) || seen.has(id)) continue;
    if (!Array.isArray(granted)) continue;
    seen.add(id);
    out.push({ id, granted: [...new Set(granted.filter(isKitPermission))] });
  }
  return out;
}

/** What a pane of the workspace hands `ptySpawn`: its kits' ids. */
export const workspaceKitIds = (ws: Pick<Workspace, "kits"> | null | undefined): string[] =>
  workspaceKits(ws).map((kit) => kit.id);

/** Why a kit a pane was given stays out of it (`pty_spawn`'s `kitIssues`). */
export interface KitLaunchIssue {
  id: string;
  reason: "missing" | "broken" | "forbidden";
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
    else notes.push(t("kits.warnBroken", { id: issue.id, detail: issue.detail ?? "" }));
  }
  if (forbidden.length) notes.push(t("kits.warnForbidden", { ids: forbidden.join(", ") }));
  return notes;
}

/*
 * The tab message protocol (docs/kits.md). A kit's tab is an isolated page
 * that talks to Hangar by postMessage; the frame that hosts it answers.
 */

/** The method a tab calls, and the permission it needs. */
export const KIT_METHODS = {
  "workspace.info": "workspace.read",
  "panes.list": "panes.read",
  "panes.send": "panes.send",
  "files.read": "files.read",
  "preview.show": "preview.show",
  notify: "notify",
} as const satisfies Record<string, KitPermission>;

export type KitMethod = keyof typeof KIT_METHODS;

/** The events a tab can listen to, and the permission each needs. */
export const KIT_EVENTS = {
  "panes.changed": "panes.read",
} as const satisfies Record<string, KitPermission>;

export type KitEventName = keyof typeof KIT_EVENTS;

/** Served by the `hangar-kit` scheme to every tab: `window.hangar`. */
export const KIT_SDK_PATH = "/_hangar/kit.js";

/** The most `files.read` returns, in bytes. */
export const KIT_FILE_MAX_BYTES = 1024 * 1024;

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

/** Whether a tab granted `granted` may call `method`; unknown methods never. */
export function kitMayCall(method: string, granted: readonly string[]): boolean {
  if (!Object.prototype.hasOwnProperty.call(KIT_METHODS, method)) return false;
  return granted.includes(KIT_METHODS[method as KitMethod]);
}
