import { asc, eq } from "drizzle-orm";

import { getDb } from "@/db";
import { comments, tasks } from "@/db/schema";
import { requireBoardAccess } from "@/lib/auth";
import { pickNextTask } from "@/lib/board-logic";
import { badRequest, json, paramRoute } from "@/lib/http";
import { serializeTask } from "@/lib/serialize";
import { isUuid } from "@/lib/validation";

export const dynamic = "force-dynamic";

type Params = { boardId: string };

/**
 * What an agent calls before claiming anything: the highest-priority
 * unassigned task in `todo` whose dependencies are all done.
 *
 * The choice is made in `pickNextTask` over the board's rows rather than in
 * SQL. Dependency eligibility is a rule that has to agree, exactly, with the
 * desktop board — expressing it once, as a tested pure function, is worth more
 * than the round trip a lateral join would save on boards this size. It is
 * also only a suggestion: nothing is reserved here, and two agents handed the
 * same task still collide on the claim, where it is decided properly.
 */
export const GET = paramRoute<Params>(async (request, context) => {
  const { boardId } = await context.params;
  if (!isUuid(boardId)) throw badRequest("boardId must be a uuid");

  await requireBoardAccess(request, boardId);
  const db = getDb();

  const rows = await db.select().from(tasks).where(eq(tasks.boardId, boardId));
  const next = pickNextTask(rows);
  if (!next) return json({ task: null });

  const taskComments = await db
    .select()
    .from(comments)
    .where(eq(comments.taskId, next.id))
    .orderBy(asc(comments.createdAt));

  return json({ task: serializeTask(next, taskComments) });
});
