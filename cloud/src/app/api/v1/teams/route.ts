import { randomUUID } from "node:crypto";
import { asc, eq } from "drizzle-orm";

import { getDb } from "@/db";
import { teamMembers, teams } from "@/db/schema";
import { requireUser } from "@/lib/auth";
import { json, readJson, route } from "@/lib/http";
import { serializeTeam } from "@/lib/serialize";
import { newTeamSchema } from "@/lib/validation";

export const dynamic = "force-dynamic";

/** Teams the caller belongs to, with the role they hold in each. */
export const GET = route(async () => {
  const user = await requireUser();
  const rows = await getDb()
    .select({ team: teams, role: teamMembers.role })
    .from(teams)
    .innerJoin(teamMembers, eq(teamMembers.teamId, teams.id))
    .where(eq(teamMembers.userId, user.id))
    .orderBy(asc(teams.createdAt));

  return json({ teams: rows.map((row) => serializeTeam(row.team, row.role)) });
});

export const POST = route(async (request) => {
  const user = await requireUser();
  const { name } = await readJson(request, newTeamSchema);

  const db = getDb();
  const now = Date.now();
  // The id is minted here rather than by the database so the team row and its
  // owner membership can go out as one batch: neon runs a batch in a
  // transaction, and a team that briefly exists with no owner would be a team
  // nobody can administer.
  const teamId = randomUUID();

  const [inserted] = await db.batch([
    db
      .insert(teams)
      .values({ id: teamId, name, createdBy: user.id, createdAt: now })
      .returning(),
    db
      .insert(teamMembers)
      .values({ teamId, userId: user.id, role: "owner", createdAt: now }),
  ]);

  const team = inserted[0];
  if (!team) throw new Error("team insert returned no row");
  return json({ team: serializeTeam(team, "owner") }, { status: 201 });
});
