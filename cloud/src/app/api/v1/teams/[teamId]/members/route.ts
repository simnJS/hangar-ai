import { and, asc, eq } from "drizzle-orm";

import { getDb } from "@/db";
import { teamMembers, users } from "@/db/schema";
import { requireTeamMembership, requireTeamOwner, requireUser } from "@/lib/auth";
import { badRequest, json, notFound, paramRoute, readJson } from "@/lib/http";
import { serializeUser } from "@/lib/serialize";
import { isUuid, newMemberSchema } from "@/lib/validation";

export const dynamic = "force-dynamic";

type Params = { teamId: string };

export const GET = paramRoute<Params>(async (_request, context) => {
  const { teamId } = await context.params;
  if (!isUuid(teamId)) throw badRequest("teamId must be a uuid");

  const user = await requireUser();
  await requireTeamMembership(teamId, user.id);

  const rows = await getDb()
    .select({ user: users, role: teamMembers.role, joinedAt: teamMembers.createdAt })
    .from(teamMembers)
    .innerJoin(users, eq(users.id, teamMembers.userId))
    .where(eq(teamMembers.teamId, teamId))
    .orderBy(asc(teamMembers.createdAt));

  return json({
    members: rows.map((row) => ({
      ...serializeUser(row.user),
      role: row.role,
      joined_at: row.joinedAt,
    })),
  });
});

/**
 * Adds someone who already has an account.
 *
 * Emailing an invitation to a stranger is a different product decision (and a
 * different Clerk flow), so this endpoint says so plainly with a 404 instead of
 * silently doing nothing: the person has to sign in once — which is what
 * populates `users.email` — before a team can point at them.
 */
export const POST = paramRoute<Params>(async (request, context) => {
  const { teamId } = await context.params;
  if (!isUuid(teamId)) throw badRequest("teamId must be a uuid");

  const actor = await requireUser();
  await requireTeamOwner(teamId, actor.id);

  const { email } = await readJson(request, newMemberSchema);
  const db = getDb();

  const [invitee] = await db
    .select()
    .from(users)
    .where(eq(users.email, email))
    .limit(1);

  if (!invitee) {
    throw notFound(`no Hangar account for ${email} yet — they must sign in once first`);
  }

  // Idempotent: adding a member twice is a double click, not an error.
  await db
    .insert(teamMembers)
    .values({ teamId, userId: invitee.id, role: "member", createdAt: Date.now() })
    .onConflictDoNothing();

  const [membership] = await db
    .select({ role: teamMembers.role, joinedAt: teamMembers.createdAt })
    .from(teamMembers)
    .where(and(eq(teamMembers.teamId, teamId), eq(teamMembers.userId, invitee.id)))
    .limit(1);

  return json(
    {
      member: {
        ...serializeUser(invitee),
        role: membership?.role ?? "member",
        joined_at: membership?.joinedAt ?? null,
      },
    },
    { status: 201 },
  );
});
