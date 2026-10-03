import { and, eq, isNull } from "drizzle-orm";

import { getDb } from "@/db";
import { boardTokens } from "@/db/schema";
import { requireBoardMembership, requireUser } from "@/lib/auth";
import { badRequest, json, notFound, paramRoute } from "@/lib/http";
import { serializeToken } from "@/lib/serialize";
import { isUuid } from "@/lib/validation";

export const dynamic = "force-dynamic";

type Params = { boardId: string; tokenId: string };

/**
 * Revocation stamps `revoked_at` instead of deleting the row: the board's
 * activity feed keeps naming a token long after someone turns it off, and a
 * deleted row would turn that history into dangling ids.
 */
export const DELETE = paramRoute<Params>(async (_request, context) => {
  const { boardId, tokenId } = await context.params;
  if (!isUuid(boardId)) throw badRequest("boardId must be a uuid");
  if (!isUuid(tokenId)) throw badRequest("tokenId must be a uuid");

  const user = await requireUser();
  await requireBoardMembership(boardId, user.id);

  const db = getDb();
  // Conditional on `revoked_at IS NULL` so a second call cannot move the
  // timestamp and rewrite when the token actually stopped working.
  const [revoked] = await db
    .update(boardTokens)
    .set({ revokedAt: Date.now() })
    .where(
      and(
        eq(boardTokens.id, tokenId),
        eq(boardTokens.boardId, boardId),
        isNull(boardTokens.revokedAt),
      ),
    )
    .returning();

  if (revoked) return json({ token: serializeToken(revoked) });

  const [existing] = await db
    .select()
    .from(boardTokens)
    .where(and(eq(boardTokens.id, tokenId), eq(boardTokens.boardId, boardId)))
    .limit(1);

  if (!existing) throw notFound("token not found");
  return json({ token: serializeToken(existing) });
});
