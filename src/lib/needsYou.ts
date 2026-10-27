import { useEffect, useMemo, useRef } from "react";
import { useAgentsIn } from "./agentState";
import { notify } from "./notify";
import { useBoardsOf } from "./paneTask";
import { NEEDS_YOU_LABEL, orchestratorOf, READY_TO_LAND_LABEL } from "./orchestration";
import type { Task } from "./board";
import type { Translator } from "../i18n";
import type { Settings, Workspace } from "../types";

/**
 * « Besoin de toi »: everything that waits for the person, across the
 * workspaces, in one list — so that a day spent letting orchestrators work is
 * not a day spent walking every workspace to see who is stuck.
 *
 * Three things wait: an agent asking for a permission or an answer (its own
 * activity, lib/agentState), a task the orchestrator or Hangar handed back
 * with a question (`needs-you`), and a delivery that passed its check and its
 * review and waits to land (`ready-to-land`, when automatic landing is off).
 * The two labels are put on the board by the orchestrator and by Hangar's
 * deliveries; this only reads them.
 *
 * Nothing of it exists until a workspace has an orchestrator or a worker:
 * someone who never pins one keeps the app exactly as it was.
 */

// A task the person has to decide on (its last comment says what about), and
// a delivery checked and reviewed, waiting for « Fusionner »: lib/orchestration.
export { NEEDS_YOU_LABEL, READY_TO_LAND_LABEL };

interface ItemBase {
  /** Stable while the item stands for the same thing, for lists and notices. */
  key: string;
  workspaceId: string;
  workspaceName: string;
}

export type NeedsYouItem =
  /** An agent in a dialog: a permission, a question. */
  | (ItemBase & { kind: "waiting"; paneId: string; paneName: string })
  /** A task labelled `needs-you`. */
  | (ItemBase & { kind: "question"; cwd: string; task: Task; orchestratorId: string | null })
  /** A task labelled `ready-to-land`. */
  | (ItemBase & { kind: "land"; cwd: string; task: Task });

export type NeedsYouKind = NeedsYouItem["kind"];

/** The order the list is read in: what blocks an agent right now comes first. */
export const KINDS: NeedsYouKind[] = ["waiting", "question", "land"];

/** A workspace where orchestration is at work: an orchestrator or a worker in it. */
export const isOrchestrated = (ws: Pick<Workspace, "panes">) =>
  ws.panes.some((pane) => pane.role === "orchestrator" || pane.role === "worker");

/**
 * The boards to read: those of the workspaces that orchestrate, once each — two
 * workspaces on one folder share a board.
 */
export function watchedBoards(workspaces: Workspace[]): string[] {
  return [...new Set(workspaces.filter(isOrchestrated).map((ws) => ws.cwd))];
}

export interface NeedsYouInput {
  /** In the order the person sees them (the navigator's). */
  workspaces: Workspace[];
  /** Panes waiting for an answer, by id, in any order. */
  waiting: readonly string[];
  /** Tasks by board folder; null while a board is still being read. */
  boards: ReadonlyMap<string, readonly Task[] | null>;
}

const newestFirst = (a: Task, b: Task) => b.updated_at - a.updated_at;

/**
 * The list, in `KINDS` order, then in the navigator's order of workspaces.
 * Questions go by priority, then freshest first; deliveries by age, the one
 * that has waited longest first. A task carrying both labels is a question:
 * whatever made it one is the thing to settle before it can land.
 */
export function needsYouItems({ workspaces, waiting, boards }: NeedsYouInput): NeedsYouItem[] {
  if (!workspaces.some(isOrchestrated)) return [];

  const asking = new Set(waiting);
  const waitingItems: NeedsYouItem[] = workspaces.flatMap((ws) =>
    ws.panes
      .filter((pane) => asking.has(pane.id))
      .map((pane) => ({
        kind: "waiting" as const,
        key: `pane:${pane.id}`,
        workspaceId: ws.id,
        workspaceName: ws.name,
        paneId: pane.id,
        paneName: pane.name,
      })),
  );

  const questions: NeedsYouItem[] = [];
  const lands: NeedsYouItem[] = [];
  const read = new Set<string>();
  for (const ws of workspaces) {
    // A shared board is listed under the first workspace that orchestrates
    // on it, once.
    if (!isOrchestrated(ws) || read.has(ws.cwd)) continue;
    read.add(ws.cwd);
    const tasks = boards.get(ws.cwd);
    if (!tasks) continue;
    const base = { workspaceId: ws.id, workspaceName: ws.name, cwd: ws.cwd };
    const open = tasks.filter((task) => task.column !== "done");
    const orchestratorId = orchestratorOf(ws)?.id ?? null;
    for (const task of [...open].sort(
      (a, b) => b.priority - a.priority || newestFirst(a, b),
    )) {
      if (task.labels.includes(NEEDS_YOU_LABEL)) {
        questions.push({
          ...base,
          kind: "question",
          key: `task:${ws.cwd}:${task.id}:${NEEDS_YOU_LABEL}`,
          task,
          orchestratorId,
        });
      }
    }
    for (const task of [...open].sort((a, b) => a.updated_at - b.updated_at)) {
      if (task.labels.includes(READY_TO_LAND_LABEL) && !task.labels.includes(NEEDS_YOU_LABEL)) {
        lands.push({
          ...base,
          kind: "land",
          key: `task:${ws.cwd}:${task.id}:${READY_TO_LAND_LABEL}`,
          task,
        });
      }
    }
  }

  return [...waitingItems, ...questions, ...lands];
}

/** What a task item says about itself: its latest comment, on one line. */
export function latestNote(task: Pick<Task, "comments">): string | null {
  const last = task.comments[task.comments.length - 1];
  const line = last?.text
    .split(/\r?\n/)
    .map((part) => part.trim())
    .find(Boolean);
  return line ?? null;
}

/**
 * What the notices have already seen: the items of the last look, and the
 * boards that had been read by then.
 */
export interface NoticeMemory {
  seen: ReadonlySet<string>;
  primed: ReadonlySet<string>;
}

export const EMPTY_MEMORY: NoticeMemory = { seen: new Set(), primed: new Set() };

/**
 * The items worth a desktop notification: tasks that were not in the list the
 * last time it was read. A board read for the first time only sets the
 * baseline — what was already waiting when the app opened is no news — and an
 * item that leaves and comes back is news again. Agents in a dialog are left
 * out: their pane already notifies (TerminalPane, notify.permission).
 */
export function noticeDelta(
  memory: NoticeMemory,
  items: NeedsYouItem[],
  loaded: ReadonlySet<string>,
): { memory: NoticeMemory; fresh: NeedsYouItem[] } {
  const fresh = items.filter(
    (item) => item.kind !== "waiting" && memory.primed.has(item.cwd) && !memory.seen.has(item.key),
  );
  return { memory: { seen: new Set(items.map((item) => item.key)), primed: loaded }, fresh };
}

/** The list for these workspaces, kept current. */
export function useNeedsYou(workspaces: Workspace[]): {
  items: NeedsYouItem[];
  boards: ReadonlyMap<string, readonly Task[] | null>;
} {
  const inUse = workspaces.some(isOrchestrated);
  // Asked for nothing while nothing orchestrates: no subscription changes
  // anything for anyone else.
  const roster = useAgentsIn(inUse ? workspaces.map((ws) => ws.id) : []);
  const boards = useBoardsOf(inUse ? watchedBoards(workspaces) : []);
  const items = useMemo(
    () => needsYouItems({ workspaces, waiting: roster.waiting, boards }),
    [workspaces, roster.waiting, boards],
  );
  return { items, boards };
}

/**
 * A desktop notification for each new question and each delivery ready to
 * land, through the same plumbing and the same settings as the panes' own:
 * off with « Notifier », and kept for when the window is not in front unless
 * asked otherwise. A click lands on the workspace's orchestrator.
 */
export function useNeedsYouNotices({
  enabled,
  items,
  boards,
  workspaces,
  settings,
  t,
}: {
  /** The main window's job alone: every window would otherwise say it once. */
  enabled: boolean;
  items: NeedsYouItem[];
  boards: ReadonlyMap<string, readonly Task[] | null>;
  workspaces: Workspace[];
  settings: Pick<Settings, "notifyOnIdle" | "notifyOnlyWhenAway">;
  t: Translator;
}) {
  const memory = useRef<NoticeMemory>(EMPTY_MEMORY);
  const live = useRef({ workspaces, settings, t });
  live.current = { workspaces, settings, t };
  const loadedKey = [...boards].filter(([, tasks]) => tasks !== null).map(([cwd]) => cwd).join("\n");
  const itemsKey = items.map((item) => item.key).join("\n");

  useEffect(() => {
    if (!enabled) return;
    const loaded = new Set(loadedKey ? loadedKey.split("\n") : []);
    const { memory: next, fresh } = noticeDelta(memory.current, items, loaded);
    memory.current = next;
    const { settings, t, workspaces } = live.current;
    if (!fresh.length || !settings.notifyOnIdle) return;
    if (settings.notifyOnlyWhenAway && document.hasFocus()) return;
    for (const item of fresh) {
      if (item.kind === "waiting") continue;
      const ws = workspaces.find((entry) => entry.id === item.workspaceId);
      const target = ws ? (orchestratorOf(ws) ?? ws.panes[0]) : null;
      notify(
        t("needsYou.notifyTitle", { workspace: item.workspaceName }),
        t(item.kind === "question" ? "needsYou.notifyQuestion" : "needsYou.notifyLand", {
          title: item.task.title,
        }),
        target ? { workspaceId: item.workspaceId, paneId: target.id } : undefined,
      );
    }
    // The list is read by its keys: a render that rebuilt the same items is
    // not a change.
  }, [enabled, itemsKey, loadedKey]);
}
