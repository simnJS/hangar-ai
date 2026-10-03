import { and, count, eq } from "drizzle-orm";

import { getDb } from "@/db";
import { teamMembers } from "@/db/schema";
import { requireTeamOwner, requireUser } from "@/lib/auth";
import { badRequest, conflict, json, notFound, paramRoute } from "@/lib/http";
import { isUuid } from "@/lib/validation";

export const dynamic = "force-dynamic";

type Params = { teamId: string; userId: string };

export const DELETE = paramRoute<Params>(async (_request, context) => {
  const { teamId, userId } = await context.params;
  if (!isUuid(teamId)) throw badRequest("teamId must be a uuid");
  if (!isUuid(userId)) throw badRequest("userId must be a uuid");

  const actor = await requireUser();
  await requireTeamOwner(teamId, actor.id);

  const db = getDb();
  const [target] = await db
    .select({ role: teamMembers.role })
    .from(teamMembers)
    .where(and(eq(teamMembers.teamId, teamId), eq(teamMembers.userId, userId)))
    .limit(1);

  if (!target) throw notFound("member not found");

  // A team with no owner is a team nobody can add anyone to, rename, or delete
  // — the removal is refused whoever asks for it, including the last owner
  // trying to walk out.
  if (target.role === "owner") {
    const [owners] = await db
      .select({ total: count() })
      .from(teamMembers)
      .where(and(eq(teamMembers.teamId, teamId), eq(teamMembers.role, "owner")));

    if ((owners?.total ?? 0) <= 1) {
      throw conflict("a team must keep at least one owner", undefined, "last_owner");
    }
  }

  await db
    .delete(teamMembers)
    .where(and(eq(teamMembers.teamId, teamId), eq(teamMembers.userId, userId)));

  return json({ removed: true });
});
