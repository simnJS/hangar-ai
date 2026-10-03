import { asc, eq } from "drizzle-orm";

import { getDb } from "@/db";
import { boards, comments, tasks } from "@/db/schema";
import { requireBoardAccess, requireBoardMembership, requireUser } from "@/lib/auth";
import { boardETag, ifNoneMatchSatisfied } from "@/lib/etag";
import { badRequest, forbidden, json, paramRoute } from "@/lib/http";
import { groupCommentsByTask, serializeTask } from "@/lib/serialize";
import { isUuid } from "@/lib/validation";

export const dynamic = "force-dynamic";

type Params = { boardId: string };

/**
 * The whole board in one payload, conditional on `boards.rev`.
 *
 * This is the endpoint agents hammer. The revision is read as part of
 * authenticating (the board row is needed either way), so a client that is
 * already up to date gets its 304 without a single task row leaving the
 * database — which is what makes a tight poll interval affordable.
 */
export const GET = paramRoute<Params>(async (request, context) => {
  const { boardId } = await context.params;
  if (!isUuid(boardId)) throw badRequest("boardId must be a uuid");

  const access = await requireBoardAccess(request, boardId);
  const etag = boardETag(access.board.rev);

  // `private, no-cache` and not `no-store`: the point is that a client *does*
  // keep the payload and revalidates it, which is what makes the 304 useful.
  // `private` keeps it out of any shared cache on the way.
  if (ifNoneMatchSatisfied(request.headers.get("if-none-match"), etag)) {
    return new Response(null, {
      status: 304,
      headers: { ETag: etag, "Cache-Control": "private, no-cache" },
    });
  }

  const db = getDb();
  const taskRows = await db
    .select()
    .from(tasks)
    .where(eq(tasks.boardId, boardId))
    .orderBy(asc(tasks.order), asc(tasks.createdAt));

  // Comments travel inside their task, like the desktop board. One join keeps
  // that from turning into a query per card.
  const commentRows = await db
    .select({ comment: comments })
    .from(comments)
    .innerJoin(tasks, eq(tasks.id, comments.taskId))
    .where(eq(tasks.boardId, boardId))
    .orderBy(asc(comments.createdAt));

  const byTask = groupCommentsByTask(commentRows.map((row) => row.comment));

  return json(
    {
      board: {
        id: access.board.id,
        name: access.board.name,
        rev: access.board.rev,
      },
      tasks: taskRows.map((task) => serializeTask(task, byTask.get(task.id) ?? [])),
    },
    { headers: { ETag: etag, "Cache-Control": "private, no-cache" } },
  );
});

/** Deleting a board takes every task, comment, token and log line with it. */
export const DELETE = paramRoute<Params>(async (_request, context) => {
  const { boardId } = await context.params;
  if (!isUuid(boardId)) throw badRequest("boardId must be a uuid");

  const user = await requireUser();
  const { role } = await requireBoardMembership(boardId, user.id);
  if (role !== "owner") throw forbidden("only a team owner can delete a board");

  await getDb().delete(boards).where(eq(boards.id, boardId));
  return json({ deleted: true });
});
