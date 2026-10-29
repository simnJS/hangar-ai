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

/**
 * remote_chat: a Claude Code pane's conversation, read from its transcript.
 * Polled every second while a chat is open; each answer is the newest page
 * unless `before` asks for an older one.
 */
export interface RemoteChatArgs {
  pane: string;
  /** A `cursor` from an earlier answer: the page just before it. */
  before?: string;
  /** Items per page. Default 60, at most 200. */
  limit?: number;
}

export interface ChatQuestionOption {
  label: string;
  description: string;
}

export interface ChatQuestion {
  question: string;
  header: string;
  multiSelect: boolean;
  options: ChatQuestionOption[];
}

/** One question's answer: the labels picked, and the person's own words. */
export interface ChatAnswer {
  selected: string[];
  other?: string;
}

export type ChatItemBody =
  /** "hangar": a message Hangar sent, which starts with "[Hangar]". */
  | { kind: "user"; text: string; from: "person" | "hangar"; images: number }
  /** Markdown as Claude wrote it. */
  | { kind: "assistant"; text: string }
  /**
   * One tool call ("Ran pnpm test", "Edited src/a.ts", "Used project_list"),
   * or reads in a row folded into one ("Read 4 files", the paths as detail).
   * detail is the input that matters, result the tool's answer, both cut to
   * a few KB. "error" also covers a call the session left without a result.
   */
  | {
      kind: "action";
      tool: string;
      title: string;
      detail?: string;
      result?: { ok: boolean; text: string };
      state: "running" | "done" | "error";
    }
  /** An AskUserQuestion; answer once answered, absent if it was refused. */
  | { kind: "question"; toolUseId: string; questions: ChatQuestion[]; answer?: ChatAnswer[] }
  /** "Conversation compacted", "Turn interrupted", "Session resumed", or an error Claude Code reported. */
  | { kind: "notice"; text: string };

/** Thinking and subagents' own steps are left out; a subagent is one action. */
export type ChatItem = { id: string; at: number } & ChatItemBody;

/** What the pane waits on the person for, from the end of the conversation. */
export type ChatPending =
  | { kind: "question"; toolUseId: string; questions: ChatQuestion[] }
  /** The pane waits and its last tool call has no result. */
  | { kind: "permission"; tool: string; title: string; detail?: string };

/**
 * Why a chat is empty when it is not the conversation's doing: the pane does
 * not run Claude Code, has no session captured yet, or its session has not
 * written a transcript yet.
 */
export type RemoteChatReason = "notClaude" | "noSession" | "noTranscript";

export interface RemoteChat {
  paneId: string;
  pane: string;
  workspace: string;
  agent: string;
  activity: RemoteActivity;
  sessionId: string | null;
  /** Oldest first. */
  items: ChatItem[];
  /** Whether there is anything before this page. */
  older: boolean;
  /** Pass as `before` for the page before; null when there is none. */
  cursor: string | null;
  pending: ChatPending | null;
  reason?: RemoteChatReason;
}

/**
 * remote_answer: answers the question a Claude Code pane waits on, typed
 * into its dialog as the person would. Refused, with nothing typed, unless
 * `toolUseId` is the question the pane waits on and its dialog is on the
 * PC's screen as Claude Code first drew it. Answers {answered: true}.
 */
export interface RemoteAnswerArgs {
  pane: string;
  /** The pending question's `toolUseId`, as remote_chat gave it. */
  toolUseId: string;
  /**
   * One per question, in order: option labels exactly as given, and/or the
   * person's own words. A single-select question takes one of the two.
   */
  answers: ChatAnswer[];
}
