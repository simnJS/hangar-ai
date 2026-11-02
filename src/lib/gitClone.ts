import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

/**
 * « Créer un espace de travail depuis un dépôt Git »: the typed contract of
 * src-tauri/src/git_clone.rs. Expected refusals come back as values (`status:
 * "refused"`), never as thrown errors; only a broken IPC throws.
 *
 * Nothing here shows anything: the flow and the UI build on it.
 */

export interface RepoRef {
  /** What is handed to git; `owner/repo` is expanded to a GitHub address. */
  url: string;
  /** Lower-case host, without port. */
  host: string;
  owner: string;
  /** Without `.git`. */
  name: string;
}

export type RepoParse =
  | ({ status: "ok" } & RepoRef)
  | { status: "refused"; reason: string };

export interface Ownership {
  mine: boolean;
  reason: string;
}

export type CloneRefusalKind =
  | "invalidUrl"
  | "invalidDestination"
  | "exists"
  | "busy"
  | "gitMissing"
  | "cancelled"
  | "timeout"
  | "failed";

export type CloneResult =
  | { status: "cloned"; path: string; reused: boolean; branch: string | null }
  | { status: "refused"; kind: CloneRefusalKind; reason: string };

export interface CloneProgress {
  jobId: string;
  /** `Receiving objects`, `Resolving deltas`… */
  phase: string;
  percent: number | null;
}

export const CLONE_PROGRESS_EVENT = "git-clone:progress";

/**
 * What the person typed or pasted, ready for `gitRepoParse`: the ends trimmed
 * (a paste brings a newline). The Rust side refuses any whitespace left inside.
 */
export function cleanRepoInput(raw: string): string {
  return raw.trim();
}

/** The folder a clone lands in by default: the repository's name. */
export function defaultCloneFolder(repo: Pick<RepoRef, "name">): string {
  return repo.name;
}

/** A job id for one clone, to follow its progress and cancel it. */
export function newCloneJobId(): string {
  const random =
    typeof crypto !== "undefined" && typeof crypto.randomUUID === "function"
      ? crypto.randomUUID()
      : `${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;
  return `clone-${random}`;
}

/** `Receiving objects 45 %`, or just the phase when git gave no percentage. */
export function progressLabel(progress: Pick<CloneProgress, "phase" | "percent">): string {
  return progress.percent === null
    ? progress.phase
    : `${progress.phase} ${progress.percent} %`;
}

/**
 * The share of the whole clone, 0 to 100, from the phase git is in. Git counts
 * each phase from 0 to 100 on its own; receiving is by far the longest, so it
 * takes the first 80 %, resolving deltas the rest and the others nothing.
 * `null` when the phase is unknown.
 */
export function overallPercent(
  progress: Pick<CloneProgress, "phase" | "percent">,
): number | null {
  if (progress.percent === null) return null;
  const percent = Math.min(100, Math.max(0, progress.percent));
  const phase = progress.phase.toLowerCase();
  if (phase.startsWith("receiving")) return Math.round(percent * 0.8);
  if (phase.startsWith("resolving")) return Math.round(80 + percent * 0.2);
  if (phase.startsWith("updating files")) return 100;
  return null;
}

export function gitRepoParse(input: string): Promise<RepoParse> {
  return invoke<RepoParse>("git_repo_parse", { input: cleanRepoInput(input) });
}

export function gitRepoOwnership(owner: string, host: string): Promise<Ownership> {
  return invoke<Ownership>("git_repo_ownership", { owner, host });
}

export function gitClone(
  url: string,
  parent: string,
  name: string,
  jobId: string,
): Promise<CloneResult> {
  return invoke<CloneResult>("git_clone", { url, parent, name, jobId });
}

/** `true` when a clone was running under this id and has been told to stop. */
export function gitCloneCancel(jobId: string): Promise<boolean> {
  return invoke<boolean>("git_clone_cancel", { jobId });
}

export type DiscardRefusalKind =
  | "invalidPath"
  | "notMade"
  | "expired"
  | "moved"
  | "link"
  | "failed";

export type DiscardResult =
  | { status: "removed" }
  | { status: "refused"; kind: DiscardRefusalKind; reason: string };

/**
 * Takes back a clone `gitClone` just made. Rust validates it: only a fresh
 * clone of this run, never a reused folder, never through a link. A refusal
 * comes back as a value; the folder is then left where it is.
 */
export function gitCloneDiscard(path: string): Promise<DiscardResult> {
  return invoke<DiscardResult>("git_clone_discard", { path });
}

/** Progress of one clone; the returned function stops listening. */
export function onCloneProgress(
  jobId: string,
  handler: (progress: CloneProgress) => void,
): Promise<UnlistenFn> {
  return listen<CloneProgress>(CLONE_PROGRESS_EVENT, (event) => {
    if (event.payload.jobId === jobId) handler(event.payload);
  });
}
