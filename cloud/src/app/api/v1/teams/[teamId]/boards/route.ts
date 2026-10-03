import { asc, eq } from "drizzle-orm";

import { getDb } from "@/db";
import { boards } from "@/db/schema";
import { requireTeamMembership, requireUser } from "@/lib/auth";
import { badRequest, json, paramRoute, readJson } from "@/lib/http";
import { serializeBoard } from "@/lib/serialize";
import { isUuid, newBoardSchema } from "@/lib/validation";

export const dynamic = "force-dynamic";

type Params = { teamId: string };

export const GET = paramRoute<Params>(async (_request, context) => {
  const { teamId } = await context.params;
  if (!isUuid(teamId)) throw badRequest("teamId must be a uuid");

  const user = await requireUser();
  await requireTeamMembership(teamId, user.id);

  const rows = await getDb()
    .select()
    .from(boards)
    .where(eq(boards.teamId, teamId))
    .orderBy(asc(boards.createdAt));

  return json({ boards: rows.map(serializeBoard) });
});

// Any member can open a board: the team is the trust boundary, not the board.
export const POST = paramRoute<Params>(async (request, context) => {
  const { teamId } = await context.params;
  if (!isUuid(teamId)) throw badRequest("teamId must be a uuid");

  const user = await requireUser();
  await requireTeamMembership(teamId, user.id);

  const { name } = await readJson(request, newBoardSchema);
  const [board] = await getDb()
    .insert(boards)
    .values({ teamId, name, rev: 0, createdAt: Date.now() })
    .returning();

  if (!board) throw new Error("board insert returned no row");
  return json({ board: serializeBoard(board) }, { status: 201 });
});
