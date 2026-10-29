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

/**
 * remote_send: typed as the person; an asleep or closed workspace is opened
 * first. A Claude Code pane not up yet — asleep, or resuming after a wake —
 * gets its text through the hangar-bridge queue, submitted once its session
 * is ready (`queued`); keys, or text without Enter, are refused there.
 */
export interface RemoteSendArgs {
  pane: string;
  text?: string;
  keys?: RemoteKey[];
  /** Press Enter after the text; the default. false pastes it without. */
  submit?: boolean;
}
export interface RemoteSent {
  sent: true;
  /** Held for a Claude Code session that is not up yet. */
  queued?: true;
}

/**
 * remote_permission: refused unless the pane is waiting. Its project asleep,
 * it is woken, and refused all the same: no dialog outlives a sleep.
 */
export interface RemotePermissionArgs {
  pane: string;
  allow: boolean;
}
export interface RemoteAnswered {
  answered: true;
  /** remote_answer to a pane not up yet: the answers wait in the queue, as a message. */
  queued?: true;
}

/**
 * remote_wake: wakes a project asleep, as a click on the PC does — its
 * agents resume their sessions. `waking` is false when it was not asleep;
 * remote_state says when it is awake.
 */
export interface RemoteWakeArgs {
  /** The project's id or name. */
  workspace: string;
}
export interface RemoteWoken {
  waking: boolean;
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

/* ── Deliveries: a worker's branch that passed and waits to land ──── */

/**
 * remote_delivery: what a worker's delivery that passed its check and its
 * review brings, read from its worktree against the branch it lands on, and
 * what the check and the review said (the board's hangar comment). Refused
 * unless the task waits for the person to land it.
 */
export interface RemoteDeliveryArgs {
  /** The board task's id. */
  task: string;
}

/** What runs for a delivery on the PC, as delivery.rs names its stages. */
export type RemoteDeliveryStage = "queued" | "merging" | "checking" | "reviewing" | "landing";

export interface RemoteDeliveryCheck {
  command: string;
  passed: boolean;
  seconds: number;
}

export interface RemoteDeliveryBlocking {
  file: string;
  line?: number;
  problem: string;
}

export interface RemoteDeliveryReview {
  approved: boolean;
  notes: string;
  blocking: RemoteDeliveryBlocking[];
}

export type RemoteDeliveryFileStatus = "added" | "modified" | "deleted" | "renamed";

export interface RemoteDeliveryFile {
  /** Relative to the repository's root, forward slashes. */
  path: string;
  /** Where a renamed file came from. */
  oldPath?: string;
  status: RemoteDeliveryFileStatus;
  /** Lines; 0 for a binary file. */
  additions: number;
  deletions: number;
  binary?: true;
}

/** A delivery of the task that failed before this one, from the board's comments. */
export interface RemoteDeliveryEarlier {
  at: number;
  /** delivery.rs's outcome: checkFailed, changesRequested, conflict, dirty, error… */
  outcome: string;
  /** Its first sentence, as the PC wrote it for the worker (English). */
  text: string;
  /** What the review asked for, when it asked for changes. */
  blocking?: RemoteDeliveryBlocking[];
}

export interface RemoteDelivery {
  taskId: string;
  title: string;
  workspace: string;
  workspaceId: string;
  /** The worker pane's name. */
  worker: string;
  branch: string;
  /** The branch it lands on. */
  target: string;
  /** What the worker said of it when it submitted; null without a summary. */
  summary: string | null;
  /** null: no check is set for the workspace. */
  check: RemoteDeliveryCheck | null;
  review: RemoteDeliveryReview | null;
  /** Untracked files are left out: only commits are delivered. */
  files: RemoteDeliveryFile[];
  /** More files changed than are listed. */
  moreFiles?: true;
  /** It waits for the person: nothing of it runs on the PC right now. */
  waiting: boolean;
  /** What runs for it now when it is not waiting: landed, or tested again first. */
  stage: RemoteDeliveryStage | null;
  /**
   * The merge it was checked and reviewed as, short. Passed back to
   * remote_land and remote_rework, which refuse once another delivery has
   * taken its place.
   */
  merge: string | null;
  /** When it passed, ms. */
  passedAt: number | null;
  /** The worker committed on its branch since: landing it is refused, its next delivery replaces it. */
  moved: boolean;
  /**
   * Local changes in the PC's checkout of the target stood in the way of
   * landing it: the files git named. null when nothing did.
   */
  inTheWay: string[] | null;
  /** Oldest first. */
  earlier: RemoteDeliveryEarlier[];
}

/** remote_delivery_file: the unified diff of one file of a delivery. */
export interface RemoteDeliveryFileArgs {
  task: string;
  /** A `path` of the delivery's `files`. */
  file: string;
}
export interface RemoteDeliveryPatch {
  path: string;
  /** Unified, as `git diff` prints it; empty for a binary file. */
  patch: string;
  binary: boolean;
  /** Cut at a line boundary past 200 KB: the rest is only on the PC. */
  truncated: boolean;
}

/**
 * remote_land: lands the delivery, as the PC's « Fusionner » does. A landing
 * whose target moved is tested again, which takes minutes: the window waits
 * a few seconds, then answers `landed: false` with its stage, and the same
 * call made again waits for that landing rather than starting another, until
 * it answers `landed: true`, or is refused with why it did not land (in
 * English, for the person: local changes in the way on the PC, the worker's
 * branch moved, the check failing…).
 */
export interface RemoteLandArgs {
  task: string;
  /** The `merge` the phone showed: refused if another delivery took its place. */
  merge?: string;
}
export type RemoteLanded =
  | { landed: true; target: string }
  | { landed: false; stage: RemoteDeliveryStage | null };

/**
 * remote_rework: the person's instruction for the worker, as their own words
 * through the queue its Claude Code session takes once ready (its project
 * woken for it), and a comment on the task that records it. What waits
 * stays until the worker's next delivery replaces it.
 */
export interface RemoteReworkArgs {
  task: string;
  text: string;
  merge?: string;
}
export interface RemoteReworked {
  sent: true;
}

/* ── Apps: an agent's dev server, opened on the phone ─────────────── */

/**
 * Where an app the phone can open comes from: a worker's port block
 * (HANGAR_PORT_BASE and the 19 after it), or a port the person added to the
 * workspace in Settings › Mobile.
 */
export type RemoteAppSource = "worker" | "custom";

/**
 * remote_apps: the dev servers the phone may open, every project's (the HQ
 * is not one), kept to the ports a server listens on right now. The mobile
 * server asks it too, before it opens one (`/m/api/app/open`).
 */
export interface RemoteAppsArgs {
  /** Only this project's, by id or name; refused when there is no such project. */
  workspace?: string;
}

export interface RemoteApp {
  workspaceId: string;
  workspace: string;
  port: number;
  source: RemoteAppSource;
  /** source "worker": its pane's name (names are unique across Hangar). */
  pane?: string;
  /** source "worker": its board task's title, when the board has it. */
  task?: string;
}

export interface RemoteApps {
  /** By project, as Hangar lists them; a worker's before the person's; by port. */
  apps: RemoteApp[];
}

/**
 * POST /m/api/app/open (not a call: the mobile server answers it, after
 * asking remote_apps). Refused with `not-allowed` (403) when remote_apps does
 * not list that port, `not-listening` (409) when nothing answers on it any
 * more, `refused` (400) with the window's message, `tailscale` (503) with
 * Tailscale's, `no-free-slot` or `pc-busy` (503).
 */
export interface RemoteAppOpenArgs {
  workspaceId: string;
  port: number;
}
export interface RemoteAppOpened {
  /** `https://<pc>.<tailnet>.ts.net:<8443…8448>/`: the app at the root of an origin of its own. */
  url: string;
  slot: number;
}

/* ── remote_stats ─────────────────────────────────────────────────── */

export type RemoteStatsPeriod = "7d" | "30d" | "90d" | "all";
/** What the series and the rankings measure. */
export type RemoteStatsMeasure = "tokens" | "cost";

/**
 * remote_stats: the agents' usage, summed up on the PC as its statistics
 * page does (lib/statsModel), all projects together: never the hourly rows
 * themselves. Refused for a period or a measure outside these.
 */
export interface RemoteStatsArgs {
  /** Whole days up to today; "all" from the first day with data. Default "30d". */
  period?: RemoteStatsPeriod;
  /** Default "tokens". */
  measure?: RemoteStatsMeasure;
}

export interface RemoteStatsTotals {
  /** Input, output, cache writes and cache reads together. */
  tokens: number;
  /** Of `tokens`, read from the prompt cache. */
  cacheRead: number;
  /** API-equivalent, US dollars: at list prices, not what a plan costs. */
  cost: number;
  activeMs: number;
  sessions: number;
  /** The person's prompts. */
  turns: number;
}

/** A row of a ranking: a project, an agent, a model. */
export interface RemoteStatsRow {
  key: string;
  /** A workspace's name, a folder's, an agent's ("Claude Code"), a model's ("Opus 5.5"). */
  label: string;
  tokens: number;
  cost: number;
  activeMs: number;
  /** Not counted for a model: 0. */
  sessions: number;
  /** Of the measure, 0–1. */
  share: number;
  /** A model the price table does not know: its tokens are not in the cost. */
  unpriced?: true;
}

/** The rows past the first ones, folded into one. */
export interface RemoteStatsOther {
  count: number;
  tokens: number;
  cost: number;
  activeMs: number;
  share: number;
}

/** Heaviest first, by the measure: six rows at most, the sixth on being "others". */
export interface RemoteStatsRanking {
  rows: RemoteStatsRow[];
  other: RemoteStatsOther | null;
}

/** One of the plan's rate-limit windows. */
export interface RemoteStatsWindow {
  /** `five_hour`, `seven_day`, or a gateway's own window. */
  kind: string;
  /** 0–100, whole. */
  percentUsed: number;
  /** When it frees up, ms; null when the session did not say. */
  resetsAt: number | null;
}

export interface RemoteStats {
  period: RemoteStatsPeriod;
  measure: RemoteStatsMeasure;
  /** When the PC read the transcripts, ms. */
  readAt: number;
  /** Read from the PC's cache alone: it is still reading what changed since. */
  cachedOnly: boolean;
  /** `to` excluded: the midnight after today, on the PC. */
  range: { from: number; to: number; days: number };
  /** A point a day; a week past 120 days. */
  granularity: "day" | "week";
  /** The first hour with anything in it, over all time; null without any data. */
  firstAt: number | null;
  totals: RemoteStatsTotals;
  /** The same length just before, for the trends; null for "all". */
  previous: RemoteStatsTotals | null;
  /** Days of the period with any active time. */
  activeDays: number;
  /** Every day (or week) of the period, quiet ones included, oldest first: the measure's value. */
  series: { at: number; value: number }[];
  projects: RemoteStatsRanking;
  agents: RemoteStatsRanking;
  models: RemoteStatsRanking;
  /** Tokens of models without a price, left out of the cost. */
  unpricedTokens: number;
  /**
   * The plan's windows as the last Claude Code session to report them saw
   * them (the hangar-bridge mod), when: one account behind every pane. null
   * until one has, or on a plan without such limits.
   */
  plan: { at: number; windows: RemoteStatsWindow[] } | null;
}
