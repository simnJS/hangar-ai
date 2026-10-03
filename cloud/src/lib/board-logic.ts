import type { ActorKind, Column } from "@/db/schema";
import type { TaskPatchInput } from "@/lib/validation";

/**
 * The board rules, with no database in sight.
 *
 * Everything here is a pure function so it can be tested directly and stay
 * honest against `src-tauri/src/board.rs`, which is the reference
 * implementation. The route handlers are thin shells around these.
 */

/**
 * Where a new card lands: at the bottom of its column.
 *
 * board.rs folds the maximum starting from 0.0, so a column whose orders are
 * all negative still yields 1 — reproduced here rather than "fixed", because
 * the two implementations have to agree on the number they produce.
 */
export function nextOrder(existingOrders: readonly number[]): number {
  return existingOrders.reduce((max, value) => Math.max(max, value), 0) + 1;
}

/** Column-typed field updates a PATCH turns into, before it reaches SQL. */
export interface TaskFieldUpdates {
  title?: string;
  description?: string;
  column?: Column;
  priority?: number;
  assignee?: string | null;
  assigneeKind?: ActorKind | null;
  labels?: string[];
  dependsOn?: string[];
  order?: number;
}

/**
 * Translates a validated patch into field updates.
 *
 * `release` wins over `assignee`: board.rs cannot tell an absent field from an
 * explicit null through serde, so handing a task back is its own flag, and a
 * payload carrying both means "release" — same order of evaluation as the Rust
 * `patch_task`, where the release branch runs last.
 */
export function taskPatchUpdates(
  patch: TaskPatchInput,
  actorKind: ActorKind,
): TaskFieldUpdates {
  const updates: TaskFieldUpdates = {};
  if (patch.title !== undefined) updates.title = patch.title;
  if (patch.description !== undefined) updates.description = patch.description;
  if (patch.column !== undefined) updates.column = patch.column;
  if (patch.priority !== undefined) updates.priority = patch.priority;
  if (patch.assignee !== undefined) {
    updates.assignee = patch.assignee;
    updates.assigneeKind = actorKind;
  }
  if (patch.release === true) {
    updates.assignee = null;
    updates.assigneeKind = null;
  }
  if (patch.labels !== undefined) updates.labels = patch.labels;
  if (patch.depends_on !== undefined) updates.dependsOn = patch.depends_on;
  if (patch.order !== undefined) updates.order = patch.order;
  return updates;
}

export function hasUpdates(updates: TaskFieldUpdates): boolean {
  return Object.keys(updates).length > 0;
}

export type ActivityAction =
  | "task.create"
  | "task.update"
  | "task.move"
  | "task.claim"
  | "task.release"
  | "task.delete"
  | "comment.create";

/**
 * Names what a patch did, so the activity feed reads like a story rather than
 * a list of "updated". A move is the event people look for, a release is the
 * one other agents care about, so both outrank a plain edit.
 */
export function activityActionForPatch(
  updates: TaskFieldUpdates,
  previousColumn: Column,
): ActivityAction {
  if (updates.column !== undefined && updates.column !== previousColumn) {
    return "task.move";
  }
  if (updates.assignee === null) return "task.release";
  return "task.update";
}

/**
 * Whether `agent` may take a task, given who holds it.
 *
 * The readable form of the predicate the claim statement runs; the statement
 * is what actually arbitrates, this is where the rule is stated once and
 * tested. A holder re-claiming its own task succeeds, exactly like
 * `claim_task` in board.rs: an agent retrying after a dropped connection must
 * not be told it lost the race to itself.
 */
export function claimIsPermitted(
  currentAssignee: string | null,
  agent: string,
): boolean {
  return currentAssignee === null || currentAssignee === agent;
}

/**
 * Where a claimed task lands.
 *
 * Only `todo` becomes `doing` — board.rs moves the column inside an
 * `if task.column == "todo"`. A task claimed while it sits in `review` is
 * being reviewed, not restarted, and dragging it back to `doing` would lose
 * that. Transcribed into the claim statement as a CASE so the whole claim
 * stays one statement.
 */
export function columnAfterClaim(column: Column): Column {
  return column === "todo" ? "doing" : column;
}

export interface NextTaskCandidate {
  id: string;
  column: Column;
  assignee: string | null;
  priority: number;
  order: number;
  dependsOn: string[];
  createdAt: number;
}

/**
 * The coordination primitive: highest-priority unassigned task in `todo` whose
 * dependencies are all done. Same rule as `next_task` in board.rs — an agent
 * calling it must never be handed work that is still blocked, or two agents
 * end up on the same item from opposite ends.
 *
 * Ties on priority go to the smallest `order`, i.e. the card sitting highest
 * in the column. Beyond that the sort falls back to creation time and id: a
 * file-backed board can lean on insertion order, a table cannot, and a
 * non-deterministic answer here would hand two agents different tasks for the
 * same board state — or the same one twice.
 */
export function pickNextTask<T extends NextTaskCandidate>(tasks: readonly T[]): T | null {
  const done = new Set(
    tasks.filter((task) => task.column === "done").map((task) => task.id),
  );

  const eligible = tasks.filter(
    (task) =>
      task.column === "todo" &&
      task.assignee === null &&
      task.dependsOn.every((dependency) => done.has(dependency)),
  );

  if (eligible.length === 0) return null;

  return [...eligible].sort(
    (a, b) =>
      b.priority - a.priority ||
      a.order - b.order ||
      a.createdAt - b.createdAt ||
      (a.id < b.id ? -1 : a.id > b.id ? 1 : 0),
  )[0]!;
}

