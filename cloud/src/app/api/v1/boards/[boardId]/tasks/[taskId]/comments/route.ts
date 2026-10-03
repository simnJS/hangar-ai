import { and, asc, eq } from "drizzle-orm";

import { getDb } from "@/db";
import { comments, tasks } from "@/db/schema";
import { requireBoardAccess } from "@/lib/auth";
import { badRequest, json, notFound, paramRoute, readJson } from "@/lib/http";
import { recordMutation } from "@/lib/mutations";
import { serializeComment } from "@/lib/serialize";
import { isUuid, newCommentSchema } from "@/lib/validation";

export const dynamic = "force-dynamic";

type Params = { boardId: string; taskId: string };

export const GET = paramRoute<Params>(async (request, context) => {
  const { boardId, taskId } = await context.params;
  if (!isUuid(boardId)) throw badRequest("boardId must be a uuid");
  if (!isUuid(taskId)) throw badRequest("taskId must be a uuid");

  await requireBoardAccess(request, boardId);
  await loadTask(boardId, taskId);

  const rows = await getDb()
    .select()
    .from(comments)
    .where(eq(comments.taskId, taskId))
    .orderBy(asc(comments.createdAt));

  return json({ comments: rows.map(serializeComment) });
});

/**
 * How agents talk to each other: decisions, blockers, what was finished. The
 * task's `updated_at` moves with it so a client sorting by recency sees the
 * conversation, exactly like the desktop board's `add_comment`.
 */
export const POST = paramRoute<Params>(async (request, context) => {
  const { boardId, taskId } = await context.params;
  if (!isUuid(boardId)) throw badRequest("boardId must be a uuid");
  if (!isUuid(taskId)) throw badRequest("taskId must be a uuid");

  const access = await requireBoardAccess(request, boardId);
  // The foreign key would catch a missing task, but not one belonging to
  // another board — that check has to be explicit.
  await loadTask(boardId, taskId);

  const input = await readJson(request, newCommentSchema);
  const now = Date.now();
  const db = getDb();

  const [inserted] = await db.batch([
    db
      .insert(comments)
      .values({
        taskId,
        author: input.author,
        authorKind: input.author_kind ?? access.actor.kind,
        authorUserId: access.user?.id ?? null,
        text: input.text,
        createdAt: now,
      })
      .returning(),
    db.update(tasks).set({ updatedAt: now }).where(eq(tasks.id, taskId)),
  ]);

  const comment = inserted[0];
  if (!comment) throw new Error("comment insert returned no row");

  const rev = await recordMutation({
    boardId,
    taskId,
    actor: access.actor.label,
    actorKind: access.actor.kind,
    action: "comment.create",
    detail: { author: input.author },
  });

  return json({ comment: serializeComment(comment), rev }, { status: 201 });
});

async function loadTask(boardId: string, taskId: string) {
  const [task] = await getDb()
    .select({ id: tasks.id })
    .from(tasks)
    .where(and(eq(tasks.id, taskId), eq(tasks.boardId, boardId)))
    .limit(1);

  if (!task) throw notFound("task not found");
  return task;
}
