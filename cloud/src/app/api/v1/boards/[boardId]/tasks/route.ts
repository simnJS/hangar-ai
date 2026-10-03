import { sql } from "drizzle-orm";

import { getDb, isUniqueViolation } from "@/db";
import { tasks } from "@/db/schema";
import { requireBoardAccess } from "@/lib/auth";
import { badRequest, conflict, json, paramRoute, readJson } from "@/lib/http";
import { recordMutation } from "@/lib/mutations";
import { serializeTask } from "@/lib/serialize";
import { isUuid, newTaskSchema } from "@/lib/validation";

export const dynamic = "force-dynamic";

type Params = { boardId: string };

export const POST = paramRoute<Params>(async (request, context) => {
  const { boardId } = await context.params;
  if (!isUuid(boardId)) throw badRequest("boardId must be a uuid");

  const access = await requireBoardAccess(request, boardId);
  const input = await readJson(request, newTaskSchema);
  const now = Date.now();
  const db = getDb();

  /**
   * A new card lands at the bottom of its column — `nextOrder` in
   * board-logic.ts, expressed as a scalar subquery so the read and the write
   * are one statement. Computing it in the handler would mean a select and an
   * insert with a gap between them, and two agents filing tasks at the same
   * moment would both read the same maximum.
   */
  const orderExpression = sql<number>`(select greatest(coalesce(max(${tasks.order}), 0), 0) + 1 from ${tasks} where ${tasks.boardId} = ${boardId} and ${tasks.column} = ${input.column})`;

  let created;
  try {
    [created] = await db
      .insert(tasks)
      .values({
        ...(input.id ? { id: input.id } : {}),
        boardId,
        title: input.title,
        description: input.description,
        column: input.column,
        priority: input.priority,
        labels: input.labels,
        dependsOn: input.depends_on,
        order: orderExpression,
        version: 0,
        createdAt: now,
        updatedAt: now,
      })
      .returning();
  } catch (error) {
    // The caller chose the id and something already has it. Anything else is
    // a real failure and keeps bubbling up to the 500.
    if (input.id && isUniqueViolation(error)) {
      throw conflict(`a task with id ${input.id} already exists`, undefined, "duplicate_id");
    }
    throw error;
  }

  if (!created) throw new Error("task insert returned no row");

  const rev = await recordMutation({
    boardId,
    taskId: created.id,
    actor: access.actor.label,
    actorKind: access.actor.kind,
    action: "task.create",
    detail: { title: created.title, column: created.column },
  });

  return json({ task: serializeTask(created), rev }, { status: 201 });
});
