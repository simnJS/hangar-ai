import { invoke } from "@tauri-apps/api/core";

export const BOARD_COLUMNS = ["todo", "doing", "review", "done"] as const;
export type BoardColumn = (typeof BOARD_COLUMNS)[number];

/** Maps a column to its translation key, so labels stay in one place. */
export const columnKey = (column: BoardColumn) => `board.${column}` as const;

export interface TaskComment {
  id: string;
  author: string;
  text: string;
  created_at: number;
}

/** Field names are snake_case to match the Rust serialization. */
export interface Task {
  id: string;
  title: string;
  description: string;
  column: BoardColumn;
  priority: number;
  assignee: string | null;
  labels: string[];
  comments: TaskComment[];
  depends_on: string[];
  created_at: number;
  updated_at: number;
  order: number;
  /** When the task last entered `doing`. Missing outside `doing`, and on
      tasks moved there before the board recorded it. */
  doing_since?: number | null;
  /** Paths and globs its worker may change (orchestration). Missing: none. */
  owns?: string[];
  /** Workers started on it so far (orchestration). Missing: none. */
  attempts?: number;
  /** Waiting on something outside the board. Missing: not blocked. A blocked
      task sits in todo, held by nobody. */
  blocked?: Blocked | null;
}

/** Why a task cannot go on. Field names follow board.rs (camelCase here). */
export interface Blocked {
  reason: string;
  /** An agent's name, or "user". */
  by: string;
  since: number;
  /** ISO day (YYYY-MM-DD) to look again. */
  recheck?: string | null;
  /** Only a person lifts it. */
  humanOnly: boolean;
  /** When the recheck day's reminder went out (human-only blocks stay). */
  reminded?: number | null;
}

/** The longest reason board.rs keeps. */
export const MAX_BLOCK_REASON = 500;

export const isBlocked = (task: Pick<Task, "blocked">): boolean => !!task.blocked;

/** Today, as the local ISO day: what board.rs compares a recheck day with. */
export function isoDay(date: Date = new Date()): string {
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`;
}

/** What is wrong with a recheck day typed for a new block; null when it is fine. */
export function recheckProblem(day: string, today: string = isoDay()): "format" | "past" | null {
  const text = day.trim();
  if (!text) return null;
  const parts = /^(\d{4})-(\d{2})-(\d{2})$/.exec(text);
  if (!parts) return "format";
  const [year, month, date] = [Number(parts[1]), Number(parts[2]), Number(parts[3])];
  const real = new Date(year, month - 1, date);
  if (real.getFullYear() !== year || real.getMonth() !== month - 1 || real.getDate() !== date) {
    return "format";
  }
  return text < today ? "past" : null;
}

/**
 * Why a card cannot leave todo, when it is blocked: the reason to show. A
 * move out of todo would leave the state behind on a task that is not waiting
 * any more, or clear it silently; the task has to be unblocked first.
 */
export function blockedMoveRefusal(task: Task, to: BoardColumn): string | null {
  if (!task.blocked || to === "todo") return null;
  return task.blocked.reason;
}

/** The patch that blocks a task from the window: the person is the caller. */
export function blockPatch(reason: string, recheck: string, humanOnly: boolean): TaskPatch {
  const day = recheck.trim();
  return {
    block: {
      reason: reason.trim(),
      ...(day ? { recheck: day } : {}),
      human_only: humanOnly,
      by: "user",
      person: true,
    },
  };
}

/** The patch that unblocks a task from the window. */
export function unblockPatch(note: string): TaskPatch {
  const text = note.trim();
  return { unblock: { by: "user", person: true, ...(text ? { note: text } : {}) } };
}

/**
 * The dependencies still holding a task back, as `next_task` sees them: any
 * id that is not a done task, including one that no longer exists — a deleted
 * dependency blocks forever, which is exactly what needs showing.
 */
export function blockersOf(task: Task, byId: Map<string, Task>): (Task | string)[] {
  return task.depends_on.flatMap((id): (Task | string)[] => {
    const dep = byId.get(id);
    if (!dep) return [id];
    return dep.column === "done" ? [] : [dep];
  });
}

export interface TaskPatch {
  title?: string;
  description?: string;
  column?: BoardColumn;
  priority?: number;
  assignee?: string;
  release?: boolean;
  labels?: string[];
  depends_on?: string[];
  order?: number;
  owns?: string[];
  /** Blocks the task (board.rs `block_task`); `person` is the window's alone. */
  block?: { reason: string; recheck?: string; human_only?: boolean; by?: string; person?: boolean };
  /** Lifts the block. */
  unblock?: { note?: string; by?: string; person?: boolean };
}

export interface NewTask {
  title: string;
  description?: string;
  column?: BoardColumn;
  priority?: number;
  labels?: string[];
  depends_on?: string[];
  owns?: string[];
}

export const boardLoad = (cwd: string) => invoke<{ tasks: Task[] }>("board_load", { cwd });

export const boardCreate = (cwd: string, task: NewTask) =>
  invoke<Task>("board_create", { cwd, task });

export const boardUpdate = (cwd: string, id: string, patch: TaskPatch) =>
  invoke<Task>("board_update", { cwd, id, patch });

export const boardDelete = (cwd: string, id: string) =>
  invoke<void>("board_delete", { cwd, id });

export const boardClaim = (cwd: string, id: string, agent: string) =>
  invoke<Task>("board_claim", { cwd, id, agent });

export const boardComment = (cwd: string, id: string, author: string, text: string) =>
  invoke<Task>("board_comment", { cwd, id, author, text });

/**
 * A worker's claim (lib/workerControl): todo to doing, one more attempt, and
 * refused — saying why — for everything that should stop it, owned files
 * overlapping a task in doing included.
 */
export const boardClaimWorker = (
  cwd: string,
  id: string,
  worker: string,
  maxAttempts: number,
  sharedFolder = false,
) => invoke<Task>("board_claim_worker", { cwd, id, worker, maxAttempts, sharedFolder });

/** Back to todo unless it waits for review or is done; `undo` takes the attempt back. */
export const boardReleaseWorker = (cwd: string, id: string, worker: string, undo: boolean) =>
  invoke<Task>("board_release_worker", { cwd, id, worker, undo });

export interface McpTarget {
  id: string;
  label: string;
  path: string;
  detected: boolean;
  configured: boolean;
  /** Registered, but under an old key or pointing at a missing executable. */
  stale: boolean;
  /** The executable the config names today, shown when it is the broken part. */
  command: string | null;
  project_scoped: boolean;
}

export interface McpReport {
  id: string;
  ok: boolean;
  message: string;
  path: string;
}

export const mcpTargets = (workspaceCwd: string) =>
  invoke<McpTarget[]>("mcp_targets", { workspaceCwd });

export const mcpInstall = (ids: string[], workspaceCwd: string) =>
  invoke<McpReport[]>("mcp_install", { ids, workspaceCwd });

export interface InstructionReport {
  path: string;
  ok: boolean;
  message: string;
}

export const writeAgentInstructions = (workspaceCwd: string) =>
  invoke<InstructionReport[]>("write_agent_instructions", { workspaceCwd });

export const agentInstructionsStatus = (workspaceCwd: string) =>
  invoke<boolean>("agent_instructions_status", { workspaceCwd });

export const mcpManualCommands = (workspaceCwd: string) =>
  invoke<{ executable: string; claude: string; codex: string; workspace: string }>(
    "mcp_manual_commands",
    { workspaceCwd },
  );
