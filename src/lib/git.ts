import { invoke } from "@tauri-apps/api/core";

/** Field names are camelCase to match the Rust serialization. */
export interface RepoInfo {
  /** git found on PATH. */
  installed: boolean;
  isRepo: boolean;
  /** Toplevel of the working tree holding the path that was asked about. */
  root: string | null;
  /** Toplevel of the main repository — the same as `root` outside a worktree. */
  mainRoot: string | null;
  isLinkedWorktree: boolean;
  /** null when the head is detached. */
  branch: string | null;
  /** Short sha. */
  head: string | null;
}

export interface Worktree {
  path: string;
  head: string | null;
  /** Short name, null when the head is detached. */
  branch: string | null;
  isMain: boolean;
  isCurrent: boolean;
  bare: boolean;
  detached: boolean;
  /** Lock reason — empty when locked without one, null when not locked. */
  locked: string | null;
  /** Why git considers the entry orphaned, null when it is healthy. */
  prunable: string | null;
}

export interface Branch {
  /** Short name — a remote branch keeps its remote prefix ("origin/xyz"). */
  name: string;
  kind: "local" | "remote";
  /** Locals only. */
  isCurrent: boolean;
  /** Where it is checked out; null when no worktree holds it. */
  worktreePath: string | null;
  /** Short sha. */
  head: string | null;
  /** Unix seconds. */
  committerDate: number | null;
  authorName: string | null;
  subject: string | null;
  /** The branch it tracks, e.g. "origin/xyz" — locals only. */
  upstream: string | null;
  /** git's own wording: "[ahead 1, behind 2]", "[gone]"… */
  track: string | null;
}

export interface RemoveResult {
  removed: boolean;
  /** Dirty or locked — ask the user, then retry with `force`. */
  needsForce: boolean;
  message: string | null;
}

export interface DeleteBranchResult {
  deleted: boolean;
  /** Not fully merged — a question for the user, not a failure. */
  needsForce: boolean;
  message: string | null;
}

export interface CopyResult {
  copied: number;
  failed: number;
}

/**
 * Never rejects for "not a repository" or "git is missing": both are states of
 * the report, so the panel can explain them instead of showing a failure.
 */
export const gitRepoInfo = (path: string) => invoke<RepoInfo>("git_repo_info", { path });

/** Every worktree attached to the repository containing `path`, main included. */
export const gitWorktreeList = (path: string) =>
  invoke<Worktree[]>("git_worktree_list", { path });

/**
 * Local and remote branches, each with the worktree already holding it, sorted
 * by descending commit date. The `HEAD` alias of a remote is left out.
 */
export const gitBranches = (path: string) => invoke<Branch[]>("git_branches", { path });

/** `baseRef` only applies to a branch being created; null starts from HEAD. */
export const gitWorktreeAdd = (args: {
  path: string;
  worktreePath: string;
  branch: string;
  createBranch: boolean;
  baseRef: string | null;
}) => invoke<void>("git_worktree_add", args);

/** Resolves with `needsForce` rather than rejecting when the tree is dirty. */
export const gitWorktreeRemove = (path: string, worktreePath: string, force: boolean) =>
  invoke<RemoveResult>("git_worktree_remove", { path, worktreePath, force });

/** Forgets the worktrees whose folder is gone. */
export const gitWorktreePrune = (path: string) =>
  invoke<void>("git_worktree_prune", { path });

/** `git fetch --all --prune`. Talks to every remote, so it can be slow. */
export const gitFetch = (path: string) => invoke<void>("git_fetch", { path });

/**
 * Resolves with `needsForce` when the branch is not fully merged: throwing the
 * commits away is the user's call, not ours.
 */
export const gitBranchDelete = (path: string, branch: string, force: boolean) =>
  invoke<DeleteBranchResult>("git_branch_delete", { path, branch, force });

/** Keeps `git worktree prune` off a worktree that only looks gone. */
export const gitWorktreeLock = (path: string, worktreePath: string) =>
  invoke<void>("git_worktree_lock", { path, worktreePath });

export const gitWorktreeUnlock = (path: string, worktreePath: string) =>
  invoke<void>("git_worktree_unlock", { path, worktreePath });

/**
 * Copies the files git ignores — a fresh checkout has none of them. Patterns
 * are relative to the root and carry no implicit globstar: ".env" is the one at
 * the root, only `**` walks into folders.
 */
export const gitCopyUntracked = (from: string, to: string, patterns: string[]) =>
  invoke<CopyResult>("git_copy_untracked", { from, to, patterns });
