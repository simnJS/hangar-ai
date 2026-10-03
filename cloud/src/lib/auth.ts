import { auth, currentUser } from "@clerk/nextjs/server";
import { and, eq, isNull, lt, or, sql } from "drizzle-orm";
import { after } from "next/server";

import { getDb } from "@/db";
import {
  boardTokens,
  boards,
  teamMembers,
  teams,
  users,
  type ActorKind,
  type BoardRow,
  type BoardTokenRow,
  type TeamRole,
  type TeamRow,
  type UserRow,
} from "@/db/schema";
import { forbidden, notFound, unauthorized } from "@/lib/http";
import {
  TOKEN_TOUCH_INTERVAL_MS,
  hashBoardToken,
  parseBearer,
  shouldTouchToken,
} from "@/lib/tokens";

/**
 * Two families of callers reach this API and they authenticate differently.
 *
 *  - People arrive with a Clerk session (the dashboard). Their identity is a
 *    row in `users`, created on first sight from the Clerk profile.
 *  - Machines arrive with `Authorization: Bearer hgr_…`. A board token is
 *    scoped to exactly one board and carries no user identity at all, so a
 *    leaked token cannot reach the rest of an account.
 *
 * Board routes accept either, which is why `requireBoardAccess` exists: the
 * dashboard and the agents read and write the same endpoints.
 */

/**
 * Resolves the signed-in Clerk user to our row, creating it on first sight.
 *
 * The common path is a single indexed select: the Clerk API is only called
 * when we have no row yet, or when the caller explicitly wants the profile
 * refreshed (`GET /me`), because that call is a network round trip we do not
 * want on every board poll.
 */
export async function requireUser(
  options: { refreshProfile?: boolean } = {},
): Promise<UserRow> {
  const { userId: clerkUserId } = await auth();
  if (!clerkUserId) throw unauthorized("sign in required");

  const db = getDb();

  if (!options.refreshProfile) {
    const [existing] = await db
      .select()
      .from(users)
      .where(eq(users.clerkUserId, clerkUserId))
      .limit(1);
    if (existing) return existing;
  }

  const profile = await currentUser();
  const email =
    profile?.primaryEmailAddress?.emailAddress ??
    profile?.emailAddresses[0]?.emailAddress ??
    null;
  const name = profile?.fullName ?? profile?.username ?? null;

  const [row] = await db
    .insert(users)
    .values({ clerkUserId, email, name, createdAt: Date.now() })
    .onConflictDoUpdate({
      target: users.clerkUserId,
      // Referencing the existing columns keeps what we already knew when Clerk
      // hands back nothing — a refresh must never blank an email out.
      set: {
        email: email ?? sql`${users.email}`,
        name: name ?? sql`${users.name}`,
      },
    })
    .returning();

  if (!row) throw new Error("user upsert returned no row");
  return row;
}

/** Membership of a team, or 404 — a team you cannot see does not exist. */
export async function requireTeamMembership(
  teamId: string,
  userId: string,
): Promise<{ team: TeamRow; role: TeamRole }> {
  const [found] = await getDb()
    .select({ team: teams, role: teamMembers.role })
    .from(teams)
    .innerJoin(
      teamMembers,
      and(eq(teamMembers.teamId, teams.id), eq(teamMembers.userId, userId)),
    )
    .where(eq(teams.id, teamId))
    .limit(1);

  if (!found) throw notFound("team not found");
  return found;
}

/**
 * Owner-only operations answer 403 rather than 404: the caller can see the
 * team, so hiding it would only be confusing.
 */
export async function requireTeamOwner(
  teamId: string,
  userId: string,
): Promise<TeamRow> {
  const { team, role } = await requireTeamMembership(teamId, userId);
  if (role !== "owner") throw forbidden("only a team owner can do that");
  return team;
}

export async function requireBoardMembership(
  boardId: string,
  userId: string,
): Promise<{ board: BoardRow; role: TeamRole }> {
  const [found] = await getDb()
    .select({ board: boards, role: teamMembers.role })
    .from(boards)
    .innerJoin(
      teamMembers,
      and(eq(teamMembers.teamId, boards.teamId), eq(teamMembers.userId, userId)),
    )
    .where(eq(boards.id, boardId))
    .limit(1);

  if (!found) throw notFound("board not found");
  return found;
}

/**
 * Validates a bearer token and checks it belongs to this board.
 *
 * Failure modes are kept apart on purpose: a bad or revoked token is a 401
 * (fix your credential), a valid token pointed at someone else's board is a
 * 403 (the credential is fine, the target is not).
 */
export async function requireBoardToken(
  request: Request,
  boardId: string,
): Promise<{ token: BoardTokenRow; board: BoardRow }> {
  const presented = parseBearer(request.headers.get("authorization"));
  if (!presented) throw unauthorized("missing bearer token");

  // The token and the board it belongs to come back together: this runs on
  // every poll, and the board's `rev` is what most of those polls are after,
  // so it is worth not paying for a second round trip.
  const [row] = await getDb()
    .select({ token: boardTokens, board: boards })
    .from(boardTokens)
    .innerJoin(boards, eq(boards.id, boardTokens.boardId))
    .where(eq(boardTokens.tokenHash, hashBoardToken(presented)))
    .limit(1);

  // Same answer for "no such token" and "malformed": a caller probing for
  // valid credentials learns nothing from the status code.
  if (!row) throw unauthorized("invalid token");

  const { token, board } = row;
  if (token.revokedAt !== null) throw unauthorized("token revoked");
  if (board.id !== boardId) throw forbidden("token is not scoped to this board");

  touchToken(token);
  return { token, board };
}

/**
 * Records that a token was used, off the response path.
 *
 * `after()` runs once the response is sent, so the poll loop of a dozen agents
 * does not pay for a write nobody is waiting on. The interval is re-checked in
 * the WHERE clause so concurrent requests collapse into one update instead of
 * racing to write the same value.
 */
function touchToken(token: BoardTokenRow): void {
  const now = Date.now();
  if (!shouldTouchToken(token.lastUsedAt, now)) return;

  const write = async () => {
    try {
      await getDb()
        .update(boardTokens)
        .set({ lastUsedAt: now })
        .where(
          and(
            eq(boardTokens.id, token.id),
            or(
              isNull(boardTokens.lastUsedAt),
              lt(boardTokens.lastUsedAt, now - TOKEN_TOUCH_INTERVAL_MS),
            ),
          ),
        );
    } catch (error) {
      // Diagnostic only: losing it must never fail the request that succeeded.
      console.error("[hangar-cloud] could not refresh token last_used_at", error);
    }
  };

  try {
    after(write);
  } catch {
    // Outside a request scope (a script, a test) `after` is unavailable; the
    // timestamp is not worth blocking on, so it is simply skipped.
  }
}

/** Who performed a mutation, as recorded in the activity feed. */
export interface BoardActor {
  label: string;
  kind: ActorKind;
}

export type BoardAccess =
  | { via: "token"; board: BoardRow; token: BoardTokenRow; user: null; actor: BoardActor }
  | { via: "session"; board: BoardRow; token: null; user: UserRow; actor: BoardActor };

/**
 * Authenticates a board request either way.
 *
 * The Authorization header decides: when a caller presents one it is judged on
 * that alone, and never silently falls back to a session it may also happen to
 * carry — a revoked token must fail, not quietly succeed as the person who is
 * still signed in on that machine.
 */
export async function requireBoardAccess(
  request: Request,
  boardId: string,
): Promise<BoardAccess> {
  if (request.headers.get("authorization")) {
    const { token, board } = await requireBoardToken(request, boardId);
    return {
      via: "token",
      board,
      token,
      user: null,
      actor: { label: token.name, kind: "agent" },
    };
  }

  const user = await requireUser();
  const { board } = await requireBoardMembership(boardId, user.id);
  return {
    via: "session",
    board,
    token: null,
    user,
    actor: { label: user.name ?? user.email ?? "user", kind: "human" },
  };
}
