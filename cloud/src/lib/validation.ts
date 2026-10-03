import { z } from "zod";

import { ACTOR_KINDS, COLUMNS } from "@/db/schema";

/**
 * The wire format is the desktop board's, field for field
 * (`src-tauri/src/board.rs`): snake_case keys, `depends_on`, `release` as the
 * way to clear an assignee. An agent that can drive the local board can drive
 * this one.
 *
 * Unknown keys are dropped rather than rejected, which is what serde does on
 * the Rust side. A model that invents an extra field gets its real fields
 * applied instead of a 400 it cannot act on.
 */

export const columnSchema = z.enum(COLUMNS);
export const actorKindSchema = z.enum(ACTOR_KINDS);

/** `u8` in board.rs — the bound is parity, not a limit anyone will reach. */
export const prioritySchema = z.number().int().min(0).max(255);

const titleSchema = z.string().trim().min(1, "title must not be empty").max(500);
const descriptionSchema = z.string().max(20_000);
const labelsSchema = z.array(z.string().trim().min(1).max(100)).max(50);
const dependsOnSchema = z.array(z.uuid()).max(200);
const actorLabelSchema = z.string().trim().min(1).max(200);

export const newTaskSchema = z.object({
  /**
   * Callers may bring their own id. That is how a client that already created
   * the task locally (offline, or on the desktop board) pushes it up without
   * ending with two rows for one piece of work.
   */
  id: z.uuid().optional(),
  title: titleSchema,
  description: descriptionSchema.default(""),
  column: columnSchema.default("todo"),
  priority: prioritySchema.default(1),
  labels: labelsSchema.default([]),
  depends_on: dependsOnSchema.default([]),
});
export type NewTaskInput = z.infer<typeof newTaskSchema>;

export const taskPatchSchema = z.object({
  title: titleSchema.optional(),
  description: descriptionSchema.optional(),
  column: columnSchema.optional(),
  priority: prioritySchema.optional(),
  assignee: actorLabelSchema.optional(),
  /** Clears the assignee. Wins over `assignee` — see `taskPatchUpdates`. */
  release: z.boolean().optional(),
  labels: labelsSchema.optional(),
  depends_on: dependsOnSchema.optional(),
  order: z.number().finite().optional(),
  /**
   * Optimistic concurrency. When present, the update only applies if the task
   * is still at that version; otherwise the caller gets a 409 carrying the
   * current task so it can rebase its change instead of overwriting someone.
   */
  expected_version: z.number().int().min(0).optional(),
});
export type TaskPatchInput = z.infer<typeof taskPatchSchema>;

export const claimSchema = z.object({
  agent: actorLabelSchema,
  kind: actorKindSchema.default("agent"),
});

export const newCommentSchema = z.object({
  author: actorLabelSchema,
  author_kind: actorKindSchema.optional(),
  text: z.string().trim().min(1).max(20_000),
});

export const activityQuerySchema = z.object({
  limit: z.coerce.number().int().min(1).max(200).default(50),
  /** Cursor: only entries strictly older than this epoch-ms timestamp. */
  before: z.coerce.number().int().min(0).optional(),
});

export const newTeamSchema = z.object({
  name: z.string().trim().min(1).max(200),
});

export const teamPatchSchema = z.object({
  name: z.string().trim().min(1).max(200),
});

export const newMemberSchema = z.object({
  email: z.email(),
});

export const newBoardSchema = z.object({
  name: z.string().trim().min(1).max(200),
});

export const newTokenSchema = z.object({
  /** Label for the machine that will hold it, e.g. "Simon's PC". */
  name: z.string().trim().min(1).max(200),
});

const uuidSchema = z.uuid();

/** Path segments are strings until proven otherwise. */
export function isUuid(value: string): boolean {
  return uuidSchema.safeParse(value).success;
}
