/**
 * The git branch a pane's header shows beside its name.
 *
 * One `git_repo_info` call per folder, however many panes sit on it: the
 * answers live in a cache keyed by folder, a call in flight is shared, and an
 * answer stays good for `PANE_BRANCH_TTL_MS`. A pane asks once on mount, again
 * every `PANE_BRANCH_REFRESH_MS`, and when it takes the focus — no file
 * watcher.
 */

import { useEffect, useState } from "react";
import { gitRepoInfo, type RepoInfo } from "./git";

/** How long an answer is trusted before the next ask goes to git. */
export const PANE_BRANCH_TTL_MS = 30_000;
/** How often a mounted pane asks again. */
export const PANE_BRANCH_REFRESH_MS = 45_000;
/** A focus right after an answer does not ask again: panes focus in bursts. */
export const PANE_BRANCH_FOCUS_MS = 2_000;

/** What the header says; null means it says nothing. */
export type PaneBranch =
  | { kind: "branch"; name: string; linkedWorktree: boolean }
  | { kind: "detached"; head: string };

/** Reads the header's branch off git's answer. Outside a repo, or without git: null. */
export function paneBranchOf(info: RepoInfo | null): PaneBranch | null {
  if (!info || !info.installed || !info.isRepo) return null;
  if (info.branch) {
    return { kind: "branch", name: info.branch, linkedWorktree: info.isLinkedWorktree };
  }
  // No branch name: a detached head, which the short sha stands for.
  return info.head ? { kind: "detached", head: info.head } : null;
}

/** The same folder written two ways is one cache entry. */
export function folderKey(folder: string): string {
  let key = folder.trim().replace(/\\/g, "/");
  while (key.length > 1 && key.endsWith("/") && !/^[A-Za-z]:\/$/.test(key)) key = key.slice(0, -1);
  // A drive path is case-insensitive.
  return /^[A-Za-z]:/.test(key) ? key.toLowerCase() : key;
}

interface Entry {
  branch: PaneBranch | null;
  /** When the answer came in; 0 until the first one. */
  at: number;
  pending: Promise<void> | null;
  listeners: Set<(branch: PaneBranch | null) => void>;
}

const cache = new Map<string, Entry>();

function entryOf(key: string): Entry {
  let entry = cache.get(key);
  if (!entry) {
    entry = { branch: null, at: 0, pending: null, listeners: new Set() };
    cache.set(key, entry);
  }
  return entry;
}

function same(a: PaneBranch | null, b: PaneBranch | null): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}

/**
 * The branch of a folder, asking git only when the cached answer is older
 * than `maxAgeMs` and no call is already on its way. A failing call reads as
 * "nothing to show".
 */
export function loadPaneBranch(
  folder: string,
  maxAgeMs: number = PANE_BRANCH_TTL_MS,
): Promise<PaneBranch | null> {
  const entry = entryOf(folderKey(folder));
  if (entry.pending) return entry.pending.then(() => entry.branch);
  if (entry.at && Date.now() - entry.at < maxAgeMs) return Promise.resolve(entry.branch);

  entry.pending = gitRepoInfo(folder)
    .then(paneBranchOf, () => null)
    .then((branch) => {
      const changed = !same(entry.branch, branch);
      entry.branch = branch;
      entry.at = Date.now();
      if (changed) entry.listeners.forEach((listener) => listener(branch));
    })
    .finally(() => {
      entry.pending = null;
    });
  return entry.pending.then(() => entry.branch);
}

/** The cached branch of a folder, without asking. */
export function peekPaneBranch(folder: string): PaneBranch | null {
  return cache.get(folderKey(folder))?.branch ?? null;
}

/** Forgets every answer. For tests. */
export function resetPaneBranchCache(): void {
  cache.clear();
}

/**
 * The branch of a pane's folder: one call on mount, then every
 * `PANE_BRANCH_REFRESH_MS`, and when the pane takes the focus. Panes on the
 * same folder share the call.
 */
export function usePaneBranch(folder: string, focused: boolean): PaneBranch | null {
  const [branch, setBranch] = useState<PaneBranch | null>(() =>
    folder ? peekPaneBranch(folder) : null,
  );

  useEffect(() => {
    if (!folder) {
      setBranch(null);
      return;
    }
    let alive = true;
    const entry = entryOf(folderKey(folder));
    const listener = (next: PaneBranch | null) => {
      if (alive) setBranch(next);
    };
    entry.listeners.add(listener);
    setBranch(entry.branch);
    const ask = () => {
      void loadPaneBranch(folder, PANE_BRANCH_TTL_MS).then((next) => {
        if (alive) setBranch(next);
      });
    };
    ask();
    const timer = window.setInterval(ask, PANE_BRANCH_REFRESH_MS);
    return () => {
      alive = false;
      entry.listeners.delete(listener);
      window.clearInterval(timer);
    };
  }, [folder]);

  // Focus: a branch switched in the shell shows as soon as the pane is looked at.
  useEffect(() => {
    if (!focused || !folder) return;
    void loadPaneBranch(folder, PANE_BRANCH_FOCUS_MS);
  }, [focused, folder]);

  return branch;
}
