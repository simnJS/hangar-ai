import { and, eq, sql, type SQL } from "drizzle-orm";

import { getDb } from "@/db";
import { tasks } from "@/db/schema";
import { requireBoardAccess } from "@/lib/auth";
import { activityActionForPatch, hasUpdates, taskPatchUpdates } from "@/lib/board-logic";
import { badRequest, conflict, json, notFound, paramRoute, readJson } from "@/lib/http";
import { recordMutation } from "@/lib/mutations";
import { serializeTask } from "@/lib/serialize";
import { isUuid, taskPatchSchema } from "@/lib/validation";

export const dynamic = "force-dynamic";

type Params = { boardId: string; taskId: string };

export const PATCH = paramRoute<Params>(async (request, context) => {
  const { boardId, taskId } = await context.params;
  if (!isUuid(boardId)) throw badRequest("boardId must be a uuid");
  if (!isUuid(taskId)) throw badRequest("taskId must be a uuid");

  const access = await requireBoardAccess(request, boardId);
  const patch = await readJson(request, taskPatchSchema);
  const updates = taskPatchUpdates(patch, access.actor.kind);

  const db = getDb();
  // Read first, but only to name what happened (a move? a release?) and to
  // tell "no such task" from "someone got there first". The update below is
  // still the only thing that decides whether the write lands, so nothing
  // rides on this row being fresh.
  const [previous] = await db
    .select()
    .from(tasks)
    .where(and(eq(tasks.id, taskId), eq(tasks.boardId, boardId)))
    .limit(1);

  if (!previous) throw notFound("task not found");

  if (!hasUpdates(updates)) {
    // An empty patch is a client bug, not a mutation: no version bump, no
    // revision bump, nothing for pollers to wake up for.
    return json({ task: serializeTask(previous), rev: access.board.rev });
  }

  const guards: SQL[] = [eq(tasks.id, taskId), eq(tasks.boardId, boardId)];
  if (patch.expected_version !== undefined) {
    // Optimistic concurrency, enforced by the statement itself: no row comes
    // back if the task moved on since the caller read it.
    guards.push(eq(tasks.version, patch.expected_version));
  }

  const [updated] = await db
    .update(tasks)
    .set({ ...updates, version: sql`${tasks.version} + 1`, updatedAt: Date.now() })
    .where(and(...guards))
    .returning();

  if (!updated) {
    const [current] = await db
      .select()
      .from(tasks)
      .where(and(eq(tasks.id, taskId), eq(tasks.boardId, boardId)))
      .limit(1);

    if (!current) throw notFound("task not found");
    throw conflict(
      `task is at version ${current.version}, not ${patch.expected_version}`,
      { current: serializeTask(current) },
      "version_conflict",
    );
  }

  const action = activityActionForPatch(updates, previous.column);
  const rev = await recordMutation({
    boardId,
    taskId,
    actor: access.actor.label,
    actorKind: access.actor.kind,
    action,
    detail:
      action === "task.move"
        ? { from: previous.column, to: updated.column }
        : { title: updated.title },
  });

  return json({ task: serializeTask(updated), rev });
});

export const DELETE = paramRoute<Params>(async (request, context) => {
  const { boardId, taskId } = await context.params;
  if (!isUuid(boardId)) throw badRequest("boardId must be a uuid");
  if (!isUuid(taskId)) throw badRequest("taskId must be a uuid");

  const access = await requireBoardAccess(request, boardId);

  // Comments go with it through the cascade; the activity trail does not, so
  // the feed still shows what the task was and who removed it.
  const [deleted] = await getDb()
    .delete(tasks)
    .where(and(eq(tasks.id, taskId), eq(tasks.boardId, boardId)))
    .returning();

  if (!deleted) throw notFound("task not found");

  const rev = await recordMutation({
    boardId,
    taskId,
    actor: access.actor.label,
    actorKind: access.actor.kind,
    action: "task.delete",
    detail: { title: deleted.title, column: deleted.column },
  });

  return json({ deleted: true, task: serializeTask(deleted), rev });
});
