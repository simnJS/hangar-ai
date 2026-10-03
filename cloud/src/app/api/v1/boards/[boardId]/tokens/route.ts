import { asc, eq } from "drizzle-orm";

import { getDb } from "@/db";
import { boardTokens } from "@/db/schema";
import { requireBoardMembership, requireUser } from "@/lib/auth";
import { badRequest, json, paramRoute, readJson } from "@/lib/http";
import { serializeToken } from "@/lib/serialize";
import { generateBoardToken, hashBoardToken } from "@/lib/tokens";
import { isUuid, newTokenSchema } from "@/lib/validation";

export const dynamic = "force-dynamic";

type Params = { boardId: string };

/**
 * Metadata only — there is no endpoint that can return a token's plaintext,
 * because the plaintext is not stored anywhere. A lost token is replaced, not
 * recovered.
 */
export const GET = paramRoute<Params>(async (_request, context) => {
  const { boardId } = await context.params;
  if (!isUuid(boardId)) throw badRequest("boardId must be a uuid");

  const user = await requireUser();
  await requireBoardMembership(boardId, user.id);

  const rows = await getDb()
    .select()
    .from(boardTokens)
    .where(eq(boardTokens.boardId, boardId))
    .orderBy(asc(boardTokens.createdAt));

  return json({ tokens: rows.map(serializeToken) });
});

/**
 * Mints a board token. Any member can: they already have full write access to
 * the board through their session, so a token adds no privilege — it only
 * hands that access to one of their machines, revocably.
 *
 * The response is the only time the plaintext exists outside the caller's
 * hands. Everything after this point works off its SHA-256.
 */
export const POST = paramRoute<Params>(async (request, context) => {
  const { boardId } = await context.params;
  if (!isUuid(boardId)) throw badRequest("boardId must be a uuid");

  const user = await requireUser();
  await requireBoardMembership(boardId, user.id);

  const { name } = await readJson(request, newTokenSchema);
  const plaintext = generateBoardToken();

  const [row] = await getDb()
    .insert(boardTokens)
    .values({
      boardId,
      name,
      tokenHash: hashBoardToken(plaintext),
      createdBy: user.id,
      createdAt: Date.now(),
    })
    .returning();

  if (!row) throw new Error("token insert returned no row");
  return json({ token: plaintext, ...serializeToken(row) }, { status: 201 });
});
