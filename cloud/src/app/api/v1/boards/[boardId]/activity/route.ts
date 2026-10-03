import { and, desc, eq, lt, type SQL } from "drizzle-orm";

import { getDb } from "@/db";
import { activity } from "@/db/schema";
import { requireBoardAccess } from "@/lib/auth";
import { badRequest, json, paramRoute, readQuery } from "@/lib/http";
import { serializeActivity } from "@/lib/serialize";
import { activityQuerySchema, isUuid } from "@/lib/validation";

export const dynamic = "force-dynamic";

type Params = { boardId: string };

/**
 * Latest first, paged with a `before` cursor rather than an offset: the feed
 * grows while it is being read, and an offset would show the same line twice.
 */
export const GET = paramRoute<Params>(async (request, context) => {
  const { boardId } = await context.params;
  if (!isUuid(boardId)) throw badRequest("boardId must be a uuid");

  await requireBoardAccess(request, boardId);
  const { limit, before } = readQuery(request, activityQuerySchema);

  const filters: SQL[] = [eq(activity.boardId, boardId)];
  if (before !== undefined) filters.push(lt(activity.createdAt, before));

  const rows = await getDb()
    .select()
    .from(activity)
    .where(and(...filters))
    .orderBy(desc(activity.createdAt))
    .limit(limit);

  return json({ activity: rows.map(serializeActivity) });
});
