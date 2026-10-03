import { eq, sql } from "drizzle-orm";

import { getDb } from "@/db";
import { activity, boards, type ActorKind } from "@/db/schema";
import type { ActivityAction } from "@/lib/board-logic";

/**
 * Concurrency model, in one place.
 *
 * The neon-http driver talks to Neon over HTTP: one round trip per statement,
 * no session, and therefore no interactive transaction to wrap a
 * read-modify-write in. That constraint shapes the whole API:
 *
 *  - Every invariant that two callers could break at once is expressed as a
 *    single conditional statement — the claim is `UPDATE … WHERE assignee IS
 *    NULL`, the versioned patch is `UPDATE … WHERE version = $expected`. The
 *    database arbitrates, not the handler, so there is no window between the
 *    check and the write.
 *  - What follows a successful mutation — bumping `boards.rev` and appending
 *    to `activity` — is deliberately *not* part of that statement. Those two
 *    are a polling hint and a journal: a crash between the mutation and this
 *    call costs a client one late refresh and one missing feed line, never a
 *    task claimed twice. Paying for that with an extra round trip on the write
 *    path would be the wrong trade.
 *
 * The two follow-ups do go out together through `db.batch`, which neon sends
 * as a single transactional request — so the feed never records something the
 * revision does not reflect.
 */
export interface MutationRecord {
  boardId: string;
  taskId?: string | null;
  actor: string;
  actorKind: ActorKind;
  action: ActivityAction;
  detail?: Record<string, unknown>;
}

/** Bumps the board revision and appends one activity line. Returns the new rev. */
export async function recordMutation(entry: MutationRecord): Promise<number | null> {
  const db = getDb();
  const now = Date.now();

  const [bumped] = await db.batch([
    db
      .update(boards)
      .set({ rev: sql`${boards.rev} + 1` })
      .where(eq(boards.id, entry.boardId))
      .returning({ rev: boards.rev }),
    db.insert(activity).values({
      boardId: entry.boardId,
      taskId: entry.taskId ?? null,
      actor: entry.actor,
      actorKind: entry.actorKind,
      action: entry.action,
      detail: entry.detail ?? null,
      createdAt: now,
    }),
  ]);

  return bumped[0]?.rev ?? null;
}
