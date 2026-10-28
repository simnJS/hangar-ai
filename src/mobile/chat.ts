import type { Activity, Chat, ChatItem, Pending } from "./api";
import type { Key } from "./i18n";

/**
 * The chat's decisions, apart from what draws them: what each item of the
 * conversation shows as, the pages the phone holds and how a poll joins
 * them, when the list follows the newest message, and the messages sent but
 * not yet in the transcript.
 */

/* ── What an item shows as ─────────────────────────────────────── */

export type ActionItem = Extract<ChatItem, { kind: "action" }>;
export type QuestionItem = Extract<ChatItem, { kind: "question" }>;
export type UserItem = Extract<ChatItem, { kind: "user" }>;

export type ActionKind = "edit" | "command" | "read" | "search" | "other";

const KINDS: Record<string, ActionKind> = {
  Edit: "edit",
  MultiEdit: "edit",
  Write: "edit",
  NotebookEdit: "edit",
  Bash: "command",
  BashOutput: "command",
  KillShell: "command",
  KillBash: "command",
  PowerShell: "command",
  Read: "read",
  NotebookRead: "read",
  LS: "read",
  WebFetch: "read",
  Grep: "search",
  Glob: "search",
  WebSearch: "search",
  ToolSearch: "search",
};

/** The icon a tool call gets: what it does to the project. */
export const actionKind = (tool: string): ActionKind => KINDS[tool] ?? "other";

/**
 * The PC writes an action's line in English: "Edited src/a.ts", "Ran pnpm
 * test", "Read 4 files". Its verb is put in the phone's words; a line in
 * another shape stays as written.
 */
const TITLES: { pattern: RegExp; key: Key }[] = [
  { pattern: /^Edited (.+)$/s, key: "action.edited" },
  { pattern: /^Wrote (.+)$/s, key: "action.wrote" },
  { pattern: /^Ran (.+)$/s, key: "action.ran" },
  { pattern: /^Read (\d+) files$/, key: "action.readFiles" },
  { pattern: /^Read (.+)$/s, key: "action.read" },
  { pattern: /^Searched for (.+)$/s, key: "action.searched" },
  { pattern: /^Used (.+)$/s, key: "action.used" },
];

type Translate = (key: Key, vars?: Record<string, string>) => string;

/** An action's line in the phone's words, what it acted on apart: a path gets its file put first. */
export interface ActionLine {
  before: string;
  subject: string;
  after: string;
}

export function actionLine(title: string, t: Translate): ActionLine {
  for (const { pattern, key } of TITLES) {
    const match = pattern.exec(title);
    if (!match) continue;
    const [before, after = ""] = t(key, { subject: "\u0000" }).split("\u0000");
    return { before, subject: match[1], after };
  }
  return { before: "", subject: title, after: "" };
}

export function actionTitle(title: string, t: Translate): string {
  const { before, subject, after } = actionLine(title, t);
  return `${before}${subject}${after}`;
}

/**
 * A path's file and folder: on a phone's width the file is what matters, so
 * it comes first and the folder after it, where the line may cut it short.
 */
export function pathParts(subject: string): { dir: string; base: string } | null {
  if (/\s/.test(subject) || /^[a-z][\w+.-]*:\/\//i.test(subject)) return null;
  const at = Math.max(subject.lastIndexOf("/"), subject.lastIndexOf("\\"));
  if (at <= 0 || at === subject.length - 1) return null;
  return { dir: subject.slice(0, at), base: subject.slice(at + 1) };
}

/** A message Hangar queued, without the "[Hangar]" it starts with. */
export const hangarText = (text: string) => text.replace(/^\s*\[Hangar\]\s*/, "");

/** A Hangar note this long opens folded, to its first lines. */
export const NOTE_FOLDS_AT = 280;
export const noteFolds = (text: string) => text.length > NOTE_FOLDS_AT || text.split("\n").length > 4;

/** A run of actions this long shows its last few, and how many came before. */
export const ACTIONS_FOLD_AT = 5;
export const ACTIONS_SHOWN = 3;

export function visibleActions(items: ActionItem[], unfolded: boolean): { hidden: number; shown: ActionItem[] } {
  if (unfolded || items.length < ACTIONS_FOLD_AT) return { hidden: 0, shown: items };
  return { hidden: items.length - ACTIONS_SHOWN, shown: items.slice(-ACTIONS_SHOWN) };
}

/** A pause this long, or a new day, puts the time above the next message. */
export const TIME_GAP_MS = 30 * 60_000;

export type ChatRow =
  | { type: "time"; key: string; at: number }
  | { type: "user"; key: string; item: UserItem }
  | { type: "hangar"; key: string; item: UserItem; text: string }
  | { type: "assistant"; key: string; text: string }
  | { type: "actions"; key: string; items: ActionItem[] }
  | { type: "question"; key: string; item: QuestionItem }
  | { type: "notice"; key: string; text: string };

const sameDay = (a: number, b: number) => new Date(a).toDateString() === new Date(b).toDateString();

/**
 * The list the chat draws, from the items: actions in a row become one
 * block, Hangar's messages notes, and the time shows after a pause. What
 * the pane waits for is drawn as its own card below, so its item in the
 * history is left out: the question asked, or the tool call that waits for
 * permission.
 */
export function chatRows(items: ChatItem[], pending: Pending | null): ChatRow[] {
  const rows: ChatRow[] = [];
  let previous: ChatItem | null = null;
  const last = items[items.length - 1];
  for (const item of items) {
    if (item.kind === "question" && pending?.kind === "question" && item.toolUseId === pending.toolUseId) continue;
    if (
      item === last &&
      item.kind === "action" &&
      item.state === "running" &&
      pending?.kind === "permission" &&
      item.tool === pending.tool
    ) {
      continue;
    }
    if ((item.kind === "assistant" || item.kind === "notice") && !item.text.trim()) continue;

    if (!previous || item.at - previous.at >= TIME_GAP_MS || !sameDay(item.at, previous.at)) {
      rows.push({ type: "time", key: `time-${item.id}`, at: item.at });
    }
    previous = item;

    const key = `${item.kind}-${item.id}`;
    switch (item.kind) {
      case "user":
        rows.push(
          item.from === "hangar" ? { type: "hangar", key, item, text: hangarText(item.text) } : { type: "user", key, item },
        );
        break;
      case "assistant":
        rows.push({ type: "assistant", key, text: item.text });
        break;
      case "action": {
        const run = rows[rows.length - 1];
        if (run?.type === "actions") run.items.push(item);
        else rows.push({ type: "actions", key, items: [item] });
        break;
      }
      case "question":
        rows.push({ type: "question", key, item });
        break;
      case "notice":
        rows.push({ type: "notice", key, text: item.text });
        break;
    }
  }
  return rows;
}

/** Whether a pane is Claude Code, which the chat shows; any other agent gets its terminal. */
export const isClaude = (agent: string | null | undefined) => agent === "claude";

/* ── The pages the phone holds ─────────────────────────────────── */

/** The first page; the polls after it ask for fewer, enough to cover a second. */
export const PAGE_LIMIT = 60;
export const POLL_LIMIT = 30;

export interface ChatLog {
  sessionId: string | null;
  /** Oldest first. */
  items: ChatItem[];
  /** There are items before the oldest held. */
  older: boolean;
  /** What to pass as `before` for the page before the oldest held. */
  cursor: string | null;
}

const fromPage = (page: Chat): ChatLog => ({
  sessionId: page.sessionId,
  items: page.items,
  older: page.older,
  cursor: page.cursor,
});

const signature = (item: ChatItem) => JSON.stringify(item);

/**
 * The latest page joined to what the phone holds. The page is the truth from
 * its first item the phone knows on: an action that finished, a question
 * answered, an item the PC folded. What it adds is counted, for the "new
 * messages" chip. A new session, or a page that shares nothing with what is
 * held, starts the log over: the phone missed too much to join them.
 * Nothing changed, the log is the same object.
 */
export function mergeLatest(log: ChatLog | null, page: Chat): { log: ChatLog; added: number; reset: boolean } {
  if (!log || log.sessionId !== page.sessionId) return { log: fromPage(page), added: 0, reset: true };
  if (!page.items.length) return { log, added: 0, reset: false };

  const known = new Map(log.items.map((item, index) => [item.id, index]));
  const first = page.items.findIndex((item) => known.has(item.id));
  if (first === -1) {
    // Nothing held yet, the page is the log; nothing in common, the phone starts over.
    const reset = log.items.length > 0;
    return { log: fromPage(page), added: reset ? 0 : page.items.length, reset };
  }

  const kept = log.items.slice(0, known.get(page.items[first].id));
  let added = 0;
  const tail = page.items.map((item) => {
    const index = known.get(item.id);
    if (index === undefined) {
      added++;
      return item;
    }
    const held = log.items[index];
    return signature(held) === signature(item) ? held : item;
  });
  const items = [...kept, ...tail];
  const same = items.length === log.items.length && items.every((item, index) => item === log.items[index]);
  return { log: same ? log : { ...log, items }, added, reset: false };
}

/**
 * An older page, asked for with `before`, put before what the phone holds.
 * One that no longer fits, the log started over or another session since,
 * is dropped.
 */
export function mergeOlder(log: ChatLog, page: Chat, before: string): { log: ChatLog; added: number } {
  if (page.sessionId !== log.sessionId || log.cursor !== before) return { log, added: 0 };
  const known = new Set(log.items.map((item) => item.id));
  const earlier = page.items.filter((item) => !known.has(item.id));
  return {
    log: { ...log, items: [...earlier, ...log.items], older: page.older, cursor: page.cursor },
    added: earlier.length,
  };
}

/** A diff's line, by what it does: added, removed, a hunk's head, or context. */
export function diffLine(line: string): "add" | "del" | "hunk" | null {
  if (line.startsWith("@@")) return "hunk";
  if (line.startsWith("+") && !line.startsWith("+++")) return "add";
  if (line.startsWith("-") && !line.startsWith("---")) return "del";
  return null;
}

/* ── Following the newest message ──────────────────────────────── */

/**
 * What changed between two renders of the list: items put above (an older
 * page), items that arrived below, or a list that shares nothing with the
 * one before (the first page, a new session).
 */
export function diffLog(previous: ChatItem[], next: ChatItem[]): { prepended: number; appended: ChatItem[]; reset: boolean } {
  const known = new Set(previous.map((item) => item.id));
  const first = next.findIndex((item) => known.has(item.id));
  if (first === -1) return { prepended: 0, appended: [], reset: next.length > 0 || previous.length > 0 };
  const fresh = next.filter((item) => !known.has(item.id));
  return { prepended: first, appended: fresh.slice(first), reset: false };
}

/** What counts in "3 new messages": what was said, not the actions between. */
export const isMessage = (item: ChatItem) => item.kind === "user" || item.kind === "assistant" || item.kind === "question";

export interface ScrollMetrics {
  scrollTop: number;
  scrollHeight: number;
  clientHeight: number;
}

/** Closer than this to the bottom counts as at it: the list follows. */
export const FOLLOW_SLACK_PX = 48;
/** Closer than this to the top, the page before is asked for. */
export const OLDER_MARGIN_PX = 320;

export const atBottom = (metrics: ScrollMetrics) =>
  metrics.scrollHeight - metrics.scrollTop - metrics.clientHeight <= FOLLOW_SLACK_PX;

export function shouldLoadOlder(metrics: ScrollMetrics, log: ChatLog | null, loading: boolean): boolean {
  return Boolean(log?.older && log.cursor !== null) && !loading && metrics.scrollTop <= OLDER_MARGIN_PX;
}

/** Where the list scrolls to once a page is put above, so what was read stays in place. */
export const anchoredScrollTop = (before: { scrollTop: number; scrollHeight: number }, scrollHeight: number) =>
  before.scrollTop + (scrollHeight - before.scrollHeight);

export interface Follow {
  /** At the bottom, the list moves with what arrives. */
  following: boolean;
  /** What arrived while the person read above. */
  unseen: number;
}

/** What items that arrived do: followed to, or their messages counted for the chip. */
export function afterNewItems(follow: Follow, arrived: ChatItem[]): Follow & { scroll: boolean } {
  if (follow.following) return { following: true, unseen: 0, scroll: arrived.length > 0 };
  return { following: false, unseen: follow.unseen + arrived.filter(isMessage).length, scroll: false };
}

/** The person scrolled: back at the bottom, nothing is unseen any more. */
export function afterScroll(follow: Follow, metrics: ScrollMetrics): Follow {
  const following = atBottom(metrics);
  return following ? { following, unseen: 0 } : { following, unseen: follow.unseen };
}

export type Chip = { key: "chat.newMessages"; n: number } | { key: "chat.toQuestion" | "chat.toPermission" | "chat.toEnd" };

/** The chip over the list, read above the bottom: what waits below first. */
export function chipOf(follow: Follow, pending: Pending | null): Chip | null {
  if (follow.following) return null;
  if (pending) return { key: pending.kind === "question" ? "chat.toQuestion" : "chat.toPermission" };
  if (follow.unseen > 0) return { key: "chat.newMessages", n: follow.unseen };
  return { key: "chat.toEnd" };
}

/* ── What the person sent ──────────────────────────────────────── */

/**
 * A message sent from the phone, shown at once at the foot of the chat: the
 * transcript holds it only once Claude Code takes it, a moment later or at
 * the end of its turn.
 */
export interface Outgoing {
  id: string;
  text: string;
  /** When it was sent, by the phone's clock. */
  at: number;
  /** The last item held when it was sent: its copy in the transcript comes after. */
  after: string | null;
}

/** Past this, a message that never showed is let go. */
export const OUTGOING_MAX_MS = 10 * 60_000;

/** The messages sent that the transcript does not hold yet. */
export function stillSending(outbox: Outgoing[], items: ChatItem[], now: number): Outgoing[] {
  const claimed = new Set<string>();
  const kept = outbox.filter((message) => {
    if (now - message.at > OUTGOING_MAX_MS) return false;
    const start = message.after === null ? 0 : items.findIndex((item) => item.id === message.after) + 1;
    const copy = items
      .slice(start)
      .find(
        (item) =>
          item.kind === "user" && item.from === "person" && !claimed.has(item.id) && item.text.trim() === message.text.trim(),
      );
    if (copy) claimed.add(copy.id);
    return !copy;
  });
  return kept.length === outbox.length ? outbox : kept;
}

/* ── What the pane waits for ───────────────────────────────────── */

/** One question or one permission, across polls. */
export function pendingKey(pending: Pending | null): string | null {
  if (!pending) return null;
  return pending.kind === "question" ? `question:${pending.toolUseId}` : `permission:${pending.tool}:${pending.title}`;
}

/** Long enough for the PC to pass the answer on and the transcript to show it. */
export const ANSWER_SETTLES_MS = 6_000;

/**
 * Whether the card for what the pane waits for shows: once answered, it
 * steps aside while the PC still reports the same thing, for a few seconds,
 * in case the pane asks again straight away.
 */
export interface Answered {
  key: string;
  at: number;
}

export function showsPending(pending: Pending | null, answered: Answered | null, now: number): boolean {
  if (!pending) return false;
  if (!answered || answered.key !== pendingKey(pending)) return true;
  return now - answered.at > ANSWER_SETTLES_MS;
}

/**
 * The pane waits for the person, and the PC cannot say what for: the chat
 * points to the terminal. Not while an answer just sent settles.
 */
export function waitsElsewhere(activity: Activity | null, pending: Pending | null, answered: Answered | null, now: number): boolean {
  return activity === "waiting" && !pending && (!answered || now - answered.at > ANSWER_SETTLES_MS);
}
