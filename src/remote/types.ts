/**
 * What the window answers to Hangar Mobile, the phone app: the JSON of the
 * remote_* internal ops, which the Rust mobile server relays and
 * lib/remoteControl answers. Shared with the server and the phone app; change
 * it only with a comment on the task that owns the contract.
 */

/** null: no agent this window knows of runs in the pane. */
export type RemoteActivity = "working" | "waiting" | "yours" | "idle" | null;

export type RemoteKey = "enter" | "escape" | "tab" | "up" | "down" | "ctrl+c";

export const REMOTE_KEYS: readonly RemoteKey[] = ["enter", "escape", "tab", "up", "down", "ctrl+c"];

export interface RemoteWaiting {
  kind: "pane" | "needsYou" | "readyToLand";
  workspaceId: string;
  workspace: string;
  /** kind "pane": the pane held by a permission or a question. */
  paneId?: string;
  pane?: string;
  /** kinds "needsYou" and "readyToLand": the board task. */
  taskId?: string;
  title?: string;
  /** Since when it waits, in ms; from the journals, else when first seen. */
  since: number;
}

export interface RemotePane {
  id: string;
  name: string;
  agent: string;
  role: "orchestrator" | "worker" | "manager" | null;
  activity: RemoteActivity;
}

export interface RemoteTask {
  id: string;
  title: string;
}

export interface RemoteProject {
  id: string;
  name: string;
  open: boolean;
  asleep: boolean;
  git: boolean | null;
  orchestrator: { name: string; activity: RemoteActivity } | null;
  panes: RemotePane[];
  board: { todo: number; doing: number; review: number; done: number };
  planStatus: string | null;
  needsYou: RemoteTask[];
  readyToLand: RemoteTask[];
}

/** remote_state {} */
export interface RemoteState {
  at: number;
  manager: { paneId: string; name: string; activity: RemoteActivity } | null;
  waiting: RemoteWaiting[];
  /** The HQ is not a project. */
  projects: RemoteProject[];
}

/** remote_screen */
export interface RemoteScreenArgs {
  /** A pane's id or name (names are unique across Hangar). */
  pane: string;
  /** Default 80. */
  lines?: number;
}
export interface RemoteScreen {
  paneId: string;
  pane: string;
  workspace: string;
  agent: string;
  activity: RemoteActivity;
  text: string;
}

/** remote_send: typed as the person; an asleep or closed workspace is opened first. */
export interface RemoteSendArgs {
  pane: string;
  text?: string;
  keys?: RemoteKey[];
  /** Press Enter after the text; the default. false pastes it without. */
  submit?: boolean;
}
export interface RemoteSent {
  sent: true;
}

/** remote_permission: refused unless the pane is waiting. */
export interface RemotePermissionArgs {
  pane: string;
  allow: boolean;
}
export interface RemoteAnswered {
  answered: true;
}

/** remote_manager: refused with "Open the project manager on the PC" without one. */
export interface RemoteManagerArgs {
  text: string;
}

/** remote_history, newest first. */
export interface RemoteHistoryArgs {
  /** Only what happened after this time (ms). */
  since?: number;
}
export interface RemoteHistoryEntry {
  at: number;
  workspace: string;
  kind: string;
  text: string;
}
export interface RemoteHistory {
  entries: RemoteHistoryEntry[];
}
