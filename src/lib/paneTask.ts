import { useEffect, useSyncExternalStore } from "react";
import { listen } from "@tauri-apps/api/event";
import { boardLoad, type Task } from "./board";
import { findAgentPane } from "./agentPane";
import type { Workspace } from "../types";

/**
 * The board of each workspace folder, loaded once and shared by everything
 * that only reads it — the inspector, the toolbar's activity line — instead of
 * each of them reading board.json on every change. The board view keeps its
 * own loader: it edits, and needs its writes back immediately.
 */

interface Entry {
  tasks: Task[] | null;
  users: number;
}

const boards = new Map<string, Entry>();
const listeners = new Set<() => void>();
let listening: Promise<void> | null = null;

function notify() {
  for (const listener of listeners) listener();
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

function refresh(cwd: string) {
  boardLoad(cwd)
    .then(({ tasks }) => {
      const entry = boards.get(cwd);
      if (!entry) return;
      entry.tasks = tasks;
      notify();
    })
    // No board yet, or no desktop shell: nothing to show rather than an error.
    .catch(() => undefined);
}

function ensureListening() {
  if (listening) return;
  listening = listen("board:changed", () => {
    for (const cwd of boards.keys()) refresh(cwd);
  })
    .then(() => undefined)
    .catch(() => {
      listening = null;
    });
}

/** Keeps the board in `cwd` loaded and current until the returned call. */
function hold(cwd: string): () => void {
  const entry = boards.get(cwd);
  if (entry) entry.users += 1;
  else boards.set(cwd, { tasks: null, users: 1 });
  ensureListening();
  refresh(cwd);
  return () => {
    const held = boards.get(cwd);
    if (held && --held.users <= 0) boards.delete(cwd);
  };
}

/** The tasks of the board in `cwd`, or null until they are read. */
export function useBoardTasks(cwd: string | null): Task[] | null {
  useEffect(() => {
    if (!cwd) return;
    return hold(cwd);
  }, [cwd]);
  return useSyncExternalStore(subscribe, () => (cwd ? (boards.get(cwd)?.tasks ?? null) : null));
}

const EMPTY_BOARDS: ReadonlyMap<string, Task[] | null> = new Map();
const boardSets = new Map<string, Map<string, Task[] | null>>();

/** The same map for as long as none of its boards changed, as a snapshot must be. */
function boardsSnapshot(key: string, cwds: readonly string[]): ReadonlyMap<string, Task[] | null> {
  if (!cwds.length) return EMPTY_BOARDS;
  const cached = boardSets.get(key);
  if (cached && cwds.every((cwd) => cached.get(cwd) === (boards.get(cwd)?.tasks ?? null))) {
    return cached;
  }
  const value = new Map(cwds.map((cwd) => [cwd, boards.get(cwd)?.tasks ?? null]));
  boardSets.set(key, value);
  return value;
}

/**
 * Several boards at once, by folder — however many there are, which a hook
 * per board could not follow. Each is null until it is read.
 */
export function useBoardsOf(cwds: readonly string[]): ReadonlyMap<string, Task[] | null> {
  const key = cwds.join("\n");
  useEffect(() => {
    if (!key) return;
    const releases = key.split("\n").map(hold);
    return () => {
      releases.forEach((release) => release());
      boardSets.delete(key);
    };
  }, [key]);
  return useSyncExternalStore(subscribe, () => boardsSnapshot(key, key ? key.split("\n") : []));
}

/** Work in progress first, then work waiting for review; the freshest of each. */
const rank = (task: Task) => (task.column === "doing" ? 0 : task.column === "review" ? 1 : 2);

/**
 * The task an agent in this pane is on, as far as the board can tell: an open
 * task whose assignee resolves to this pane (lib/agentPane, which only
 * answers when the name is unambiguous). Done and unassigned tasks never
 * count.
 */
export function taskForPane(
  tasks: Task[] | null,
  paneId: string,
  workspaces: Workspace[],
  activeWorkspaceId: string | null,
): Task | null {
  if (!tasks) return null;
  const mine = tasks.filter((task) => {
    if (!task.assignee || (task.column !== "doing" && task.column !== "review")) return false;
    return findAgentPane(task.assignee, workspaces, activeWorkspaceId)?.paneId === paneId;
  });
  mine.sort((a, b) => rank(a) - rank(b) || b.updated_at - a.updated_at);
  return mine[0] ?? null;
}
