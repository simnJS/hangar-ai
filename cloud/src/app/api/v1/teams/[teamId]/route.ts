import { eq } from "drizzle-orm";

import { getDb } from "@/db";
import { teams } from "@/db/schema";
import { requireTeamOwner, requireUser } from "@/lib/auth";
import { badRequest, json, paramRoute, readJson } from "@/lib/http";
import { serializeTeam } from "@/lib/serialize";
import { isUuid, teamPatchSchema } from "@/lib/validation";

export const dynamic = "force-dynamic";

type Params = { teamId: string };

export const PATCH = paramRoute<Params>(async (request, context) => {
  const { teamId } = await context.params;
  if (!isUuid(teamId)) throw badRequest("teamId must be a uuid");

  const user = await requireUser();
  await requireTeamOwner(teamId, user.id);

  const { name } = await readJson(request, teamPatchSchema);
  const [updated] = await getDb()
    .update(teams)
    .set({ name })
    .where(eq(teams.id, teamId))
    .returning();

  if (!updated) throw new Error("team update returned no row");
  return json({ team: serializeTeam(updated, "owner") });
});
