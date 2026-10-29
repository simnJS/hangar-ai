import { useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getPaneActivity } from "./agentState";
import type { BridgeEvent, RateLimit } from "./bridge";
import { runControl, type ControlDeps } from "./control";
import { activityHistory, startActivityLog, textOf, type ActivityHistory } from "./activityLog";
import { gitWorktreeList } from "./git";
import { managerJournal, type ManagerEvent } from "./managerEvents";
import { managerOf } from "./orchestration";
import { loadProjects, projectsDeps } from "./projects";
import { choicesOf, NOT_ON_SCREEN, pendingQuestions, typeAnswers, type Choice } from "./remoteAnswer";
import { usageStats, type UsageStats } from "./stats";
import { PERIODS, projectFor, summarize, workspaceRoots, type Measure } from "./statsModel";
import { isMainWindow } from "./windows";
import { systemLocale, translate, type TranslateKey, type TranslateParams } from "../i18n";
import { askedQuestions, describeCall } from "../remote/calls";
import { remoteStatsOf } from "../remote/stats";
import {
  REMOTE_KEYS,
  type ChatItem,
  type ChatPending,
  type ChatQuestion,
  type RemoteActivity,
  type RemoteAnswered,
  type RemoteChat,
  type RemoteChatReason,
  type RemoteHistory,
  type RemoteKey,
  type RemoteScreen,
  type RemoteSent,
  type RemoteState,
  type RemoteStats,
  type RemoteStatsMeasure,
  type RemoteStatsPeriod,
  type RemoteWaiting,
  type RemoteWoken,
} from "../remote/types";
import { AGENTS, type AppState, type Pane, type Workspace } from "../types";

/**
 * What the window answers to Hangar Mobile (the phone app): the remote_*
 * internal requests the Rust mobile server relays as `control:internal`. The
 * person is the one asking, from their phone, so what is sent is typed as
 * them — agents' pane control being turned off does not stop it.
 *
 * Claude Code's permission prompt takes one key: Enter accepts the highlighted
 * "1. Yes", Escape is "No, and tell Claude what to do differently". The
 * hangar-bridge mod reports what the call would do (permission {waiting,
 * tool, input}), never which choices the prompt offers, so those two keys are
 * all that is sent. A question (AskUserQuestion) is answered through its
 * dialog too, by lib/remoteAnswer.
 */

const SCREEN_DEFAULT = 80;
const NO_MANAGER = "Open the project manager on the PC";
/**
 * A closed or asleep workspace gets this long to start a pane's shell for a
 * send: within the 10 s the mobile server gives the window, past which the
 * phone tries the send again.
 */
const OPEN_WAIT_MS = 8_000;
const OPEN_POLL_MS = 250;
/** Past this, a Claude Code woken from the phone that never said hello is taken to run without the mod. */
const RESUME_MS = 120_000;

const same = (a: string, b: string) => a.trim().toLowerCase() === b.trim().toLowerCase();

const activityOf = (paneId: string): RemoteActivity => getPaneActivity(paneId)?.activity ?? null;

/** A pane by id or by name, wherever it is. */
function findPane(state: AppState, ref: unknown): { ws: Workspace; pane: Pane } {
  const wanted = typeof ref === "string" ? ref.trim() : "";
  if (!wanted) throw new Error("Name the pane, by id or by name.");
  for (const ws of state.workspaces) {
    const pane = ws.panes.find((entry) => entry.id === wanted);
    if (pane) return { ws, pane };
  }
  for (const ws of state.workspaces) {
    const pane = ws.panes.find((entry) => same(entry.name, wanted));
    if (pane) return { ws, pane };
  }
  throw new Error(`No pane "${wanted}" in Hangar.`);
}

/** A project by id or by name. */
function findWorkspace(state: AppState, ref: unknown): Workspace {
  const wanted = typeof ref === "string" ? ref.trim() : "";
  if (!wanted) throw new Error("Name the project, by id or by name.");
  const ws =
    state.workspaces.find((entry) => entry.id === wanted) ??
    state.workspaces.find((entry) => same(entry.name, wanted));
  if (!ws) throw new Error(`No project "${wanted}" in Hangar.`);
  return ws;
}

/** Since when something waits, when the journals cannot say: the first time it was seen. */
const firstSeen = new Map<string, number>();

function since(key: string, journaled: number | undefined, at: number): number {
  if (journaled !== undefined) {
    firstSeen.set(key, journaled);
    return journaled;
  }
  const known = firstSeen.get(key);
  if (known !== undefined) return known;
  firstSeen.set(key, at);
  return at;
}

/** The latest time the manager's journal recorded an event of this kind. */
function journaledAt(match: (event: ManagerEvent) => boolean): number | undefined {
  let latest: number | undefined;
  for (const entry of managerJournal()) {
    if (match(entry.event)) latest = Math.max(latest ?? 0, entry.at);
  }
  return latest;
}

async function remoteState(deps: ControlDeps): Promise<RemoteState> {
  const at = Date.now();
  const state = deps.snapshot();
  const summaries = await loadProjects(deps.projects ?? projectsDeps(deps.snapshot, deps.isOpen));

  const waiting: RemoteWaiting[] = [];
  const seen = new Set<string>();
  for (const ws of state.workspaces) {
    for (const pane of ws.panes) {
      if (pane.agent === "shell" || activityOf(pane.id) !== "waiting") continue;
      const key = `pane:${pane.id}`;
      seen.add(key);
      waiting.push({
        kind: "pane",
        workspaceId: ws.id,
        workspace: ws.name,
        paneId: pane.id,
        pane: pane.name,
        since: since(key, journaledAt((e) => e.kind === "paneWaiting" && e.paneId === pane.id), at),
      });
    }
  }
  for (const project of summaries) {
    for (const [kind, tasks] of [
      ["needsYou", project.needsYou],
      ["readyToLand", project.readyToLand],
    ] as const) {
      for (const task of tasks) {
        const key = `${kind}:${project.id}:${task.id}`;
        seen.add(key);
        waiting.push({
          kind,
          workspaceId: project.id,
          workspace: project.name,
          taskId: task.id,
          title: task.title,
          since: since(
            key,
            journaledAt((e) => e.kind === (kind === "needsYou" ? "taskNeedsYou" : "readyToLand") && e.taskId === task.id),
            at,
          ),
        });
      }
    }
  }
  for (const key of [...firstSeen.keys()]) if (!seen.has(key)) firstSeen.delete(key);
  waiting.sort((a, b) => a.since - b.since);

  const manager = managerOf(state);
  return {
    at,
    manager: manager
      ? { paneId: manager.pane.id, name: manager.pane.name, activity: activityOf(manager.pane.id) }
      : null,
    waiting,
    projects: summaries.map((project) => {
      const ws = state.workspaces.find((entry) => entry.id === project.id);
      return {
        id: project.id,
        name: project.name,
        open: project.open,
        asleep: project.asleep,
        git: project.git,
        orchestrator: project.orchestrator,
        panes: (ws?.panes ?? []).map((pane) => ({
          id: pane.id,
          name: pane.name,
          agent: pane.agent,
          role: pane.role ?? null,
          activity: activityOf(pane.id),
        })),
        board: project.board,
        planStatus: project.planStatus,
        needsYou: project.needsYou,
        readyToLand: project.readyToLand,
      };
    }),
  };
}

async function remoteScreen(deps: ControlDeps, args: Record<string, unknown>): Promise<RemoteScreen> {
  const { ws, pane } = findPane(deps.snapshot(), args.pane);
  const lines = typeof args.lines === "number" ? args.lines : SCREEN_DEFAULT;
  const screen = {
    paneId: pane.id,
    pane: pane.name,
    workspace: ws.name,
    agent: pane.agent,
    activity: activityOf(pane.id),
  };
  try {
    const read = (await runControl("pane_read", { workspace: ws.id, pane: pane.id, lines }, deps)) as {
      text: string;
    };
    return { ...screen, text: read.text };
  } catch (err) {
    // Asleep or closed, it shows nothing yet: no reason for the phone to call it gone.
    if (await deps.terminal(pane.id)) throw err;
    return { ...screen, text: "" };
  }
}

/**
 * Claude Code panes of a workspace woken from the phone, by id, since when:
 * their shells start at once and Claude Code resumes in them seconds later,
 * so until the mod says hello, what is typed lands in the shell. Their
 * hangar-bridge events take them off (noteBridgeEvent).
 */
const resuming = new Map<string, number>();

/** The hangar-bridge queue drains only where the mod runs (Settings › Agents). */
const queues = (deps: ControlDeps) => deps.snapshot().settings.claudeBridge !== false;

/**
 * Wakes a workspace asleep, or opens one closed, as a click on the PC does:
 * its terminals mount, each Claude Code resuming its session, and it counts
 * as used, so hibernation leaves it alone for its whole delay.
 */
function wake(deps: ControlDeps, ws: Workspace) {
  const starting = deps.asleep(ws.id) || !deps.isOpen(ws.id);
  deps.openWorkspace(ws.id);
  deps.touch(ws.id);
  if (!starting || !queues(deps)) return;
  const at = Date.now();
  for (const pane of ws.panes) {
    if (pane.agent === "claude" && !resuming.has(pane.id) && !getPaneActivity(pane.id)?.bridged) {
      resuming.set(pane.id, at);
    }
  }
}

/**
 * A Claude Code pane not up yet: no terminal — its workspace asleep or
 * closed — or one woken from the phone whose Claude Code has not said hello.
 */
async function resumes(deps: ControlDeps, pane: Pane): Promise<boolean> {
  if (pane.agent !== "claude") return false;
  if (!(await deps.terminal(pane.id))) return true;
  const since = resuming.get(pane.id);
  if (since === undefined) return false;
  if (getPaneActivity(pane.id)?.bridged || Date.now() - since >= RESUME_MS) {
    resuming.delete(pane.id);
    return false;
  }
  return true;
}

/** Nothing was sent to a Claude Code not up yet, said before `wake` changes why. */
function notUp(deps: ControlDeps, ws: Workspace, pane: Pane): Error {
  const why = deps.asleep(ws.id)
    ? `"${ws.name}" was asleep: it is waking up now, its agents resuming their sessions.`
    : !deps.isOpen(ws.id)
      ? `"${ws.name}" was not open: its panes are starting now.`
      : `"${pane.name}" is resuming its session.`;
  return new Error(`${why} Nothing was sent; send it again in a few seconds.`);
}

/**
 * What the person sent a Claude Code not up yet, put in the hangar-bridge
 * queue, which the session takes once it is ready, as their own words; the
 * workspace is woken for it. Without text to queue, or without the mod to
 * take it, nothing is sent.
 */
async function hold(deps: ControlDeps, ws: Workspace, pane: Pane, text: string | null): Promise<{ queued: true }> {
  if (text === null || !queues(deps)) {
    const error = notUp(deps, ws, pane);
    wake(deps, ws);
    throw error;
  }
  await deps.enqueue(pane.id, text);
  wake(deps, ws);
  return { queued: true };
}

/**
 * Typed as the person: pane_send's path, without when_idle.
 *
 * A Claude Code pane that is not up — its workspace asleep or closed, or
 * resuming after the phone woke it — has a shell first and Claude Code
 * seconds later: typed then, the text runs in the shell, or reaches no
 * process at all. It is held in the queue instead (`hold`); keys, or text
 * left unsubmitted, cannot wait there. Any other pane is typed into once its
 * terminal and its shell are up.
 */
async function type(
  deps: ControlDeps,
  ws: Workspace,
  pane: Pane,
  input: { text?: string; keys?: string[]; submit?: boolean },
): Promise<RemoteSent> {
  if (await resumes(deps, pane)) {
    const text = input.text ?? "";
    const holdable = text.trim() !== "" && !input.keys?.length && input.submit !== false;
    return { sent: true, ...(await hold(deps, ws, pane, holdable ? text : null)) };
  }
  if (!(await deps.terminal(pane.id))) {
    wake(deps, ws);
    // A terminal mounts before its shell is spawned: what it takes until
    // then reaches no process.
    const up = async () => Boolean(await deps.terminal(pane.id)) && (await deps.alive(pane.id));
    for (let waited = 0; waited < OPEN_WAIT_MS; waited += OPEN_POLL_MS) {
      await deps.sleep(OPEN_POLL_MS);
      if (await up()) break;
    }
  }
  await runControl(
    "pane_send",
    { workspace: ws.id, pane: pane.id, text: input.text, keys: input.keys, submit: input.submit },
    deps,
  );
  return { sent: true };
}

async function remoteSend(deps: ControlDeps, args: Record<string, unknown>): Promise<RemoteSent> {
  const { ws, pane } = findPane(deps.snapshot(), args.pane);
  const keys = Array.isArray(args.keys) ? args.keys.map((key) => String(key).toLowerCase()) : [];
  const bad = keys.find((key) => !REMOTE_KEYS.includes(key as RemoteKey));
  if (bad) throw new Error(`Unknown key "${bad}". One of: ${REMOTE_KEYS.join(", ")}.`);
  return type(deps, ws, pane, {
    text: typeof args.text === "string" ? args.text : undefined,
    keys,
    submit: args.submit === false ? false : undefined,
  });
}

/** remote_wake: wakes a project asleep; one awake is left as it is. */
async function remoteWake(deps: ControlDeps, args: Record<string, unknown>): Promise<RemoteWoken> {
  const ws = findWorkspace(deps.snapshot(), args.workspace);
  if (!deps.asleep(ws.id)) return { waking: false };
  wake(deps, ws);
  return { waking: true };
}

async function remotePermission(deps: ControlDeps, args: Record<string, unknown>): Promise<RemoteAnswered> {
  if (typeof args.allow !== "boolean") throw new Error('Say whether to allow: "allow" is true or false.');
  const { ws, pane } = findPane(deps.snapshot(), args.pane);
  if (activityOf(pane.id) !== "waiting") {
    // A sleep stops the agents: no dialog outlives it, and Enter or Escape
    // typed while it resumes would reach the shell.
    if (deps.asleep(ws.id)) {
      wake(deps, ws);
      throw new Error(
        `"${ws.name}" was asleep, so "${pane.name}" no longer waits for a permission. ` +
          "It is waking up now; nothing was sent.",
      );
    }
    throw new Error(`"${pane.name}" is not waiting for a permission: nothing was sent.`);
  }
  await type(deps, ws, pane, { keys: [args.allow ? "enter" : "escape"] });
  return { answered: true };
}

async function remoteManager(deps: ControlDeps, args: Record<string, unknown>): Promise<RemoteSent> {
  const text = typeof args.text === "string" ? args.text : "";
  if (!text.trim()) throw new Error("Nothing to send: give text.");
  const manager = managerOf(deps.snapshot());
  if (!manager) throw new Error(NO_MANAGER);
  return type(deps, manager.ws, manager.pane, { text });
}

/**
 * What happened, from the log kept on disk (lib/activityLog), newest first:
 * the journals in memory forget with every restart, the disk does not.
 */
async function remoteHistory(
  deps: ControlDeps,
  args: Record<string, unknown>,
  history: ActivityHistory,
): Promise<RemoteHistory> {
  const from = typeof args.since === "number" && args.since > 0 ? args.since : 0;
  const locale = deps.snapshot().settings.locale ?? systemLocale();
  const say = (key: TranslateKey, params?: TranslateParams) =>
    translate(locale, key, params);
  const entries = await history.read(from);
  return {
    entries: entries
      .filter((entry) => entry.at > from)
      .map((entry) => ({ ...entry, text: textOf(entry, say) }))
      .sort((a, b) => b.at - a.at),
  };
}

/** A page of a transcript, as the chat_page command reads it (chat/ in Rust). */
export interface ChatPage {
  items: ChatItem[];
  older: boolean;
  cursor: string | null;
  /** An AskUserQuestion still without its answer. */
  question: { toolUseId: string; questions: ChatQuestion[] } | null;
  /** The last tool call, when it has no result yet. */
  openCall: { tool: string; title: string; detail?: string } | null;
}

/** null: the session has written no transcript yet. */
export type ChatReader = (args: {
  sessionId: string;
  cwd: string;
  before?: string;
  limit?: number;
}) => Promise<ChatPage | null>;

const readTranscript: ChatReader = (args) => invoke<ChatPage | null>("chat_page", args);

/** A call a pane's mod reported started and not yet ended. */
interface LiveCall {
  toolUseId: string;
  tool: string;
  input: Record<string, unknown>;
  at: number;
}

/**
 * What a pane's hangar-bridge mod reports that its transcript cannot say yet:
 * Claude Code writes a call there only once it has its result, so neither a
 * call running, nor the question or permission the pane waits on, is in it.
 */
interface Live {
  /** Oldest first. */
  calls: LiveCall[];
  /** The permission dialog open, with the input of the call it asks about. */
  permission: { tool: string; input: unknown } | null;
}

const live = new Map<string, Live>();

/**
 * The plan's rate-limit windows, from whichever session reported last, as
 * the navigator shows them (lib/agentState): one account behind every pane.
 */
let planLimits: { limits: RateLimit[]; at: number } | null = null;

/** Keeps what a pane's mod reports of its calls and dialogs, and of the plan (bridge:event). */
export function noteBridgeEvent(paneId: string, event: BridgeEvent, at = Date.now()) {
  // Its Claude Code is up, or has quit: either way, no longer resuming.
  resuming.delete(paneId);
  if (event.kind === "usage") {
    // A session without such limits (an API key) says nothing of the plan.
    if (event.rateLimits.length) planLimits = { limits: event.rateLimits, at };
    return;
  }
  if (event.kind === "hello" || event.kind === "session" || event.kind === "turn" || event.kind === "end") {
    // A turn that starts or ends, a conversation that starts over: nothing
    // from before still runs or waits.
    live.delete(paneId);
    return;
  }
  if (event.kind !== "tool" && event.kind !== "permission") return;
  const pane = live.get(paneId) ?? { calls: [], permission: null };
  if (event.kind === "permission") {
    pane.permission = event.waiting && event.tool ? { tool: event.tool, input: event.input } : null;
  } else {
    pane.calls = pane.calls.filter((call) => call.toolUseId !== event.toolUseId);
    if (event.phase === "start") pane.calls.push({ toolUseId: event.toolUseId, tool: event.tool, input: event.input ?? {}, at });
  }
  if (pane.calls.length === 0 && !pane.permission) live.delete(paneId);
  else live.set(paneId, pane);
}

/** The question a pane's AskUserQuestion call puts to the person, while it runs. */
function liveQuestion(paneId: string): { toolUseId: string; questions: ChatQuestion[] } | null {
  const calls = live.get(paneId)?.calls ?? [];
  for (let at = calls.length - 1; at >= 0; at--) {
    const call = calls[at];
    if (call.tool !== "AskUserQuestion") continue;
    const questions = askedQuestions(call.input);
    if (questions.length > 0) return { toolUseId: call.toolUseId, questions };
  }
  return null;
}

/**
 * What the pane waits on the person for: what its mod reports first, then
 * what the transcript says — a question without its answer, or the last call
 * without a result while the pane waits.
 */
function pendingOf(paneId: string, page: ChatPage, activity: RemoteActivity, cwd: string | null): ChatPending | null {
  const question = liveQuestion(paneId) ?? page.question;
  if (question) return { kind: "question", ...question };
  if (activity !== "waiting") return null;
  const permission = live.get(paneId)?.permission;
  if (permission) {
    const { ask, detail } = describeCall(permission.tool, permission.input, cwd);
    return { kind: "permission", tool: permission.tool, title: ask, ...(detail ? { detail } : {}) };
  }
  if (page.openCall) return { kind: "permission", ...page.openCall };
  return null;
}

/**
 * The calls the pane runs now, as actions after the transcript's: not a
 * question (the pending card shows it), nor the call a permission dialog
 * holds, which has not run yet.
 */
function runningOf(paneId: string, page: ChatPage, pending: ChatPending | null, cwd: string | null): ChatItem[] {
  const known = new Set(page.items.map((item) => item.id));
  const calls = (live.get(paneId)?.calls ?? []).filter(
    (call) => call.tool !== "AskUserQuestion" && !known.has(call.toolUseId),
  );
  if (pending?.kind === "permission") {
    for (let at = calls.length - 1; at >= 0; at--) {
      if (calls[at].tool !== pending.tool) continue;
      calls.splice(at, 1);
      break;
    }
  }
  return calls.map((call) => {
    const { title, detail } = describeCall(call.tool, call.input, cwd);
    return {
      id: call.toolUseId,
      at: call.at,
      kind: "action",
      tool: call.tool,
      title,
      ...(detail ? { detail } : {}),
      state: "running",
    };
  });
}

async function remoteChat(
  deps: ControlDeps,
  args: Record<string, unknown>,
  read: ChatReader,
): Promise<RemoteChat> {
  const { ws, pane } = findPane(deps.snapshot(), args.pane);
  const before = args.before;
  if (before !== undefined && typeof before !== "string") {
    throw new Error('"before" is a cursor from an earlier answer.');
  }
  const activity = activityOf(pane.id);
  const chat = {
    paneId: pane.id,
    pane: pane.name,
    workspace: ws.name,
    agent: pane.agent,
    activity,
    sessionId: pane.sessionId,
  };
  const empty = (reason: RemoteChatReason): RemoteChat => ({
    ...chat,
    items: [],
    older: false,
    cursor: null,
    pending: null,
    reason,
  });
  if (pane.agent !== "claude") return empty("notClaude");
  if (!pane.sessionId) return empty("noSession");
  const cwd = pane.cwd || ws.cwd;
  const page = await read({
    sessionId: pane.sessionId,
    cwd,
    ...(typeof before === "string" ? { before } : {}),
    ...(typeof args.limit === "number" ? { limit: args.limit } : {}),
  });
  if (!page) return empty("noTranscript");
  const pending = pendingOf(pane.id, page, activity, cwd);
  return {
    ...chat,
    // What runs now comes last: on the newest page alone.
    items: typeof before === "string" ? page.items : [...page.items, ...runningOf(pane.id, page, pending, cwd)],
    older: page.older,
    cursor: page.cursor,
    pending,
  };
}

/** Panes an answer is being typed into: a second one would type over it. */
const answering = new Set<string>();

/** Lines read to find a question's dialog: four questions' review fits well within. */
const ANSWER_SCREEN = 120;

/**
 * The answers as the person would write them, for a session that resumed
 * after a sleep: its dialog went with the agent the sleep stopped.
 */
function answerText(questions: ChatQuestion[], choices: Choice[]): string {
  return questions
    .map((question, index) => {
      const { picked, other } = choices[index];
      const said = [...picked.map((at) => question.options[at].label), ...(other ? [other] : [])];
      return `${question.question}\n→ ${said.join(", ")}`;
    })
    .join("\n\n");
}

/**
 * Answers the question a Claude Code pane waits on, through its dialog on the
 * PC (lib/remoteAnswer): only the one its mod reports open, or its
 * transcript shows unanswered, so a phone showing an older one is refused.
 * A pane not up yet — asleep, or resuming — gets the answers in the queue.
 */
async function remoteAnswer(
  deps: ControlDeps,
  args: Record<string, unknown>,
  read: ChatReader,
): Promise<RemoteAnswered> {
  const { ws, pane } = findPane(deps.snapshot(), args.pane);
  const toolUseId = typeof args.toolUseId === "string" ? args.toolUseId : "";
  if (!toolUseId) throw new Error('Name the question: its "toolUseId", from the chat.');
  if (pane.agent !== "claude" || !pane.sessionId) {
    throw new Error(`"${pane.name}" has no Claude Code question to answer.`);
  }
  if (answering.has(pane.id)) throw new Error(`An answer is being typed into "${pane.name}" already.`);
  answering.add(pane.id);
  try {
    const page = await read({ sessionId: pane.sessionId, cwd: pane.cwd || ws.cwd });
    const questions = pendingQuestions(liveQuestion(pane.id) ?? page?.question ?? null, page?.items ?? [], toolUseId);
    const choices = choicesOf(questions, args.answers);
    if (await resumes(deps, pane)) {
      return { answered: true, ...(await hold(deps, ws, pane, answerText(questions, choices))) };
    }
    if (!(await deps.terminal(pane.id))) throw new Error(NOT_ON_SCREEN);
    const at = { workspace: ws.id, pane: pane.id };
    await typeAnswers(
      {
        read: async () =>
          ((await runControl("pane_read", { ...at, lines: ANSWER_SCREEN }, deps)) as { text: string }).text,
        type: async (step) => {
          await runControl("pane_send", { ...at, text: step.text, keys: step.keys, submit: false }, deps);
        },
        sleep: deps.sleep,
      },
      questions,
      choices,
    );
    return { answered: true };
  } finally {
    answering.delete(pane.id);
  }
}

/** A scan this recent is still the truth: another period or measure does not redo it. */
const STATS_FRESH_MS = 60_000;
/**
 * A scan longer than this — the first after a launch reads every transcript —
 * is answered from the cache instead, within the 10 s the mobile server gives
 * the window; it carries on for the next call.
 */
const STATS_WAIT_MS = 6_000;

/** The usage statistics, as stats.rs reads them; see `usageReader`. */
export type UsageReader = () => Promise<UsageStats>;

/** Where remote_stats reads from; the tests bring their own. */
export interface StatsSources {
  usage: UsageReader;
  /** The worktrees of a workspace's repository: their sessions are the workspace's. */
  worktrees: (cwd: string) => Promise<string[]>;
  plan: () => { limits: RateLimit[]; at: number } | null;
  now: () => number;
}

/**
 * The statistics a scan at a time: the last full one while it is fresh, a
 * call during a scan joining it, and the cache's answer when the scan takes
 * longer than `waitMs`.
 */
export function usageReader(
  read: (args: { cachedOnly?: boolean }) => Promise<UsageStats>,
  {
    freshMs = STATS_FRESH_MS,
    waitMs = STATS_WAIT_MS,
    now = Date.now,
    wait = (ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms)),
  }: { freshMs?: number; waitMs?: number; now?: () => number; wait?: (ms: number) => Promise<void> } = {},
): UsageReader {
  let last: { data: UsageStats; at: number } | null = null;
  let scan: Promise<UsageStats> | null = null;
  return async () => {
    if (last && now() - last.at < freshMs) return last.data;
    if (!scan) {
      const started = read({}).then((data) => {
        last = { data, at: now() };
        return data;
      });
      scan = started;
      started
        .finally(() => {
          if (scan === started) scan = null;
        })
        .catch(() => undefined);
    }
    const done = await Promise.race([scan, wait(waitMs).then(() => null)]);
    return done ?? last?.data ?? read({ cachedOnly: true });
  };
}

/** Worktrees change seldom: git is asked again after this. */
const WORKTREES_FRESH_MS = 5 * 60_000;
const worktreesSeen = new Map<string, { at: number; paths: string[] }>();

async function worktreesOf(cwd: string): Promise<string[]> {
  const known = worktreesSeen.get(cwd);
  if (known && Date.now() - known.at < WORKTREES_FRESH_MS) return known.paths;
  // Not a repository, or no git: the workspace's own folders say enough.
  const paths = await gitWorktreeList(cwd).then(
    (list) => list.map((tree) => tree.path),
    () => [],
  );
  worktreesSeen.set(cwd, { at: Date.now(), paths });
  return paths;
}

export const statsSources: StatsSources = {
  usage: usageReader(usageStats),
  worktrees: worktreesOf,
  plan: () => planLimits,
  now: () => Date.now(),
};

const MEASURES: readonly RemoteStatsMeasure[] = ["tokens", "cost"];

function choice<T extends string>(value: unknown, options: readonly T[], fallback: T, name: string): T {
  if (value === undefined || value === null) return fallback;
  if (typeof value === "string" && options.includes(value as T)) return value as T;
  throw new Error(`Unknown ${name} "${String(value)}". One of: ${options.join(", ")}.`);
}

const agentLabel = (agent: string) => AGENTS.find((entry) => entry.id === agent)?.label ?? agent;

/**
 * remote_stats: what the statistics page shows, summed up the page's way
 * (lib/statsModel) over every project, for a phone to draw as it comes.
 */
async function remoteStats(deps: ControlDeps, args: Record<string, unknown>, sources: StatsSources): Promise<RemoteStats> {
  const period = choice<RemoteStatsPeriod>(args.period, PERIODS, "30d", "period");
  const measure: Measure = choice(args.measure, MEASURES, "tokens", "measure");
  const { workspaces } = deps.snapshot();
  const [data, worktrees] = await Promise.all([
    sources.usage(),
    Promise.all(
      workspaces.map(async (ws) => [ws.id, await sources.worktrees(ws.cwd).catch(() => [])] as const),
    ).then((pairs) => Object.fromEntries(pairs)),
  ]);
  const roots = workspaceRoots(workspaces, worktrees);
  const summary = summarize(data, {
    period,
    project: null,
    measure,
    now: sources.now(),
    projects: data.sessions.map((session) => projectFor(session.cwd, roots)),
    agentLabel,
    topSessions: 0,
  });
  return remoteStatsOf(summary, {
    period,
    measure,
    readAt: data.generatedAt,
    cachedOnly: data.scan.cachedOnly,
    plan: sources.plan(),
  });
}

/** Every op this module answers; any other is not its to answer. */
export const isRemoteOp = (op: string) => op.startsWith("remote_");

export async function runRemote(
  op: string,
  args: Record<string, unknown>,
  deps: ControlDeps,
  history: ActivityHistory = activityHistory,
  chat: ChatReader = readTranscript,
  stats: StatsSources = statsSources,
): Promise<unknown> {
  // The person asks: not the agents' switch (agentPaneControl) to refuse.
  const asPerson: ControlDeps = { ...deps, enabled: () => true };
  switch (op) {
    case "remote_state":
      return remoteState(asPerson);
    case "remote_screen":
      return remoteScreen(asPerson, args);
    case "remote_send":
      return remoteSend(asPerson, args);
    case "remote_permission":
      return remotePermission(asPerson, args);
    case "remote_manager":
      return remoteManager(asPerson, args);
    case "remote_history":
      return remoteHistory(asPerson, args, history);
    case "remote_chat":
      return remoteChat(asPerson, args, chat);
    case "remote_answer":
      return remoteAnswer(asPerson, args, chat);
    case "remote_wake":
      return remoteWake(asPerson, args);
    case "remote_stats":
      return remoteStats(asPerson, args, stats);
    default:
      throw new Error(`Unknown remote request "${op}".`);
  }
}

interface InternalRequest {
  id: string;
  op: string;
  args: Record<string, unknown>;
}

type Send = (id: string, ok: boolean, value: unknown, error: string | null) => Promise<void>;

const reply: Send = (id, ok, value, error) =>
  invoke<void>("control_reply", { id, ok, value: value ?? null, error });

/** Answers one internal request if it is a remote_* one; false when it is not. */
export function answerRemote(request: InternalRequest, deps: ControlDeps | null, send: Send = reply): boolean {
  if (!isRemoteOp(request.op)) return false;
  const run = deps
    ? runRemote(request.op, request.args ?? {}, deps)
    : Promise.reject(new Error("Hangar.AI is still starting. Try again in a moment."));
  run
    .then((value) => send(request.id, true, value, null))
    .catch((err) => send(request.id, false, null, err instanceof Error ? err.message : String(err)))
    .catch(() => undefined);
  return true;
}

/**
 * Answers Hangar Mobile's requests for as long as the main window is up.
 * `deps` is read when a request lands; null means the store has not loaded.
 */
export function useRemoteControl(deps: ControlDeps | null) {
  const depsRef = useRef(deps);
  depsRef.current = deps;
  const loaded = deps !== null;

  // The history is written from the moment the store has loaded.
  useEffect(() => {
    if (!isMainWindow || !loaded) return;
    return startActivityLog(() => depsRef.current!.snapshot());
  }, [loaded]);

  useEffect(() => {
    if (!isMainWindow) return;
    let cancelled = false;
    const offs: (() => void)[] = [];
    const keep = (stop: () => void) => {
      if (cancelled) stop();
      else offs.push(stop);
    };
    listen<InternalRequest>("control:internal", (event) => {
      answerRemote(event.payload, depsRef.current);
    })
      .then(keep)
      .catch(() => undefined);
    listen<{ paneId: string; event: BridgeEvent }>("bridge:event", (event) => {
      noteBridgeEvent(event.payload.paneId, event.payload.event);
    })
      .then(keep)
      .catch(() => undefined);
    return () => {
      cancelled = true;
      for (const off of offs) off();
    };
  }, []);
}
