import { and, eq, isNull, or, sql } from "drizzle-orm";

import { getDb } from "@/db";
import { tasks } from "@/db/schema";
import { requireBoardAccess } from "@/lib/auth";
import { claimIsPermitted } from "@/lib/board-logic";
import { badRequest, conflict, json, notFound, paramRoute, readJson } from "@/lib/http";
import { recordMutation } from "@/lib/mutations";
import { serializeTask } from "@/lib/serialize";
import { claimSchema, isUuid } from "@/lib/validation";

export const dynamic = "force-dynamic";

type Params = { boardId: string; taskId: string };

/**
 * The invariant the whole service exists for.
 *
 * Several agents poll the same board and go for the same task within
 * milliseconds of each other. Exactly one may win, and the arbitration happens
 * inside a single statement — `UPDATE … WHERE assignee IS NULL OR assignee =
 * :agent` — so there is no window between checking and taking. No application
 * lock could offer that across serverless invocations, and the neon-http
 * driver has no interactive transaction to fall back on.
 *
 * The predicate matches `claim_task` in board.rs down to the re-claim: the
 * holder taking its own task again succeeds, so an agent whose connection
 * dropped mid-call can simply retry. Everyone else gets a 409 naming the
 * owner, which is their cue to move to the next task.
 */
export const POST = paramRoute<Params>(async (request, context) => {
  const { boardId, taskId } = await context.params;
  if (!isUuid(boardId)) throw badRequest("boardId must be a uuid");
  if (!isUuid(taskId)) throw badRequest("taskId must be a uuid");

  const access = await requireBoardAccess(request, boardId);
  const { agent, kind } = await readJson(request, claimSchema);

  const db = getDb();
  const [claimed] = await db
    .update(tasks)
    .set({
      assignee: agent,
      assigneeKind: kind,
      // `columnAfterClaim`, transcribed so the whole claim stays one
      // statement: only a task waiting in `todo` starts moving. One picked up
      // in `review` is being reviewed, not restarted.
      column: sql`case when ${tasks.column} = 'todo' then 'doing' else ${tasks.column} end`,
      version: sql`${tasks.version} + 1`,
      updatedAt: Date.now(),
    })
    .where(
      and(
        eq(tasks.id, taskId),
        eq(tasks.boardId, boardId),
        or(isNull(tasks.assignee), eq(tasks.assignee, agent)),
      ),
    )
    .returning();

  if (!claimed) {
    // Only now, on the failure path, is it worth a second round trip to say
    // *why* nothing was claimed.
    const [current] = await db
      .select()
      .from(tasks)
      .where(and(eq(tasks.id, taskId), eq(tasks.boardId, boardId)))
      .limit(1);

    if (!current) throw notFound("task not found");

    // The row was held when the update ran; if it reads free now, the holder
    // let go in between. Saying "already claimed by null" would be nonsense —
    // tell the caller to come back instead.
    if (claimIsPermitted(current.assignee, agent)) {
      throw conflict(
        "the task changed hands while claiming it — try again",
        { owner: current.assignee },
        "already_claimed",
      );
    }

    throw conflict(
      `already claimed by ${current.assignee}`,
      { owner: current.assignee },
      "already_claimed",
    );
  }

  const rev = await recordMutation({
    boardId,
    taskId,
    actor: access.actor.label,
    actorKind: access.actor.kind,
    action: "task.claim",
    detail: { agent, title: claimed.title },
  });

  return json({ task: serializeTask(claimed), rev });
});
