import { useSyncExternalStore } from "react";
import type { ContextGauge } from "./context";
import type { QueuedPrompt } from "./ipc";

/**
 * Everything a pane knows about the agent in it, beyond the four-way state of
 * lib/agentState: what the inspector, the toolbar's activity line and the
 * palette show about the focused pane.
 *
 * The pane is the only writer — it is the one hearing from the hangar-bridge
 * mod and reading the transcripts — and anything else only reads. Like
 * agentState it lives outside React, because the readers are nowhere near the
 * pane in the tree. Each window keeps its own: an inspector only ever shows a
 * pane its own window draws.
 */

export type PaneEventKind =
  | "session"
  | "turn-start"
  | "turn-end"
  | "permission"
  | "edit"
  | "queued"
  | "dropped"
  | "exit";

export interface PaneEvent {
  /** Epoch milliseconds. */
  at: number;
  kind: PaneEventKind;
  /** One line, already worded for display. */
  text: string;
}

export interface PaneInsight {
  /** The model the agent runs, as the mod or the transcript names it. */
  model: string | null;
  /** The agent's own version, from the mod's hello. */
  agentVersion: string | null;
  /** When this pane's current session started or was resumed. */
  sessionSince: number | null;
  context: ContextGauge | null;
  /** What the session has cost so far, when the agent reports it. */
  costUsd: number | null;
  /** Prompts held until the agent is free (see bridgeEnqueue). */
  queue: QueuedPrompt[];
  /** Files the agent edited, most recent first, each listed once. */
  editedFiles: string[];
  /** What happened in the pane, most recent first. */
  events: PaneEvent[];
  /** First line of the agent's last answer. */
  lastAnswer: string | null;
  /** When the running turn started; null between turns. */
  turnStartedAt: number | null;
}

/** Enough history to read back over a coffee, not a transcript. */
export const MAX_EVENTS = 40;
export const MAX_EDITED_FILES = 50;

export const EMPTY_INSIGHT: PaneInsight = {
  model: null,
  agentVersion: null,
  sessionSince: null,
  context: null,
  costUsd: null,
  queue: [],
  editedFiles: [],
  events: [],
  lastAnswer: null,
  turnStartedAt: null,
};

const insights = new Map<string, PaneInsight>();
const listeners = new Set<() => void>();

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

function set(paneId: string, next: PaneInsight) {
  insights.set(paneId, next);
  for (const listener of listeners) listener();
}

const current = (paneId: string) => insights.get(paneId) ?? EMPTY_INSIGHT;

function sameGauge(a: ContextGauge | null, b: ContextGauge | null) {
  if (a === b) return true;
  if (!a || !b) return false;
  return a.pct === b.pct && a.usedTokens === b.usedTokens && a.window === b.window && a.model === b.model;
}

function sameQueue(a: QueuedPrompt[], b: QueuedPrompt[]) {
  return a.length === b.length && a.every((entry, i) => entry.id === b[i].id && entry.text === b[i].text);
}

/**
 * Merges what changed. A patch that changes nothing notifies nobody, so the
 * pane can report on every event without re-rendering the inspector for it.
 */
export function updatePaneInsight(paneId: string, patch: Partial<PaneInsight>) {
  const before = current(paneId);
  let changed = false;
  for (const key of Object.keys(patch) as (keyof PaneInsight)[]) {
    const a = before[key];
    const b = patch[key];
    if (b === undefined) continue;
    const same =
      key === "context"
        ? sameGauge(a as ContextGauge | null, b as ContextGauge | null)
        : key === "queue"
          ? sameQueue(a as QueuedPrompt[], b as QueuedPrompt[])
          : Object.is(a, b);
    if (!same) {
      changed = true;
      break;
    }
  }
  if (!changed) return;
  set(paneId, { ...before, ...patch });
}

/** Adds a line to the pane's history, newest first. */
export function recordPaneEvent(paneId: string, kind: PaneEventKind, text: string, at = Date.now()) {
  const before = current(paneId);
  const events = [{ at, kind, text }, ...before.events].slice(0, MAX_EVENTS);
  set(paneId, { ...before, events });
}

/** Moves a file to the top of the pane's edited files, listing it once. */
export function recordEditedFile(paneId: string, file: string) {
  const before = current(paneId);
  if (before.editedFiles[0] === file) return;
  const editedFiles = [file, ...before.editedFiles.filter((f) => f !== file)].slice(
    0,
    MAX_EDITED_FILES,
  );
  set(paneId, { ...before, editedFiles });
}

/** Forgets a pane: it closed, or restarted under a new id. */
export function clearPaneInsight(paneId: string) {
  if (!insights.delete(paneId)) return;
  for (const listener of listeners) listener();
}

export function getPaneInsight(paneId: string): PaneInsight {
  return current(paneId);
}

/** The pane's insight, or the empty one for a pane that never reported. */
export function usePaneInsight(paneId: string | null): PaneInsight {
  return useSyncExternalStore(subscribe, () => (paneId ? current(paneId) : EMPTY_INSIGHT));
}
