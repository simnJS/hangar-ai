import {
  bigint,
  doublePrecision,
  index,
  integer,
  jsonb,
  pgTable,
  primaryKey,
  text,
  uniqueIndex,
  uuid,
} from "drizzle-orm/pg-core";

/**
 * Columns of a board, in order. Same four as the desktop app
 * (`src-tauri/src/board.rs`) — the wire format is shared, so this list may
 * never drift from it.
 */
export const COLUMNS = ["todo", "doing", "review", "done"] as const;
export type Column = (typeof COLUMNS)[number];

/** Who did something: a person signed in through Clerk, or a board token. */
export const ACTOR_KINDS = ["human", "agent"] as const;
export type ActorKind = (typeof ACTOR_KINDS)[number];

export const TEAM_ROLES = ["owner", "member"] as const;
export type TeamRole = (typeof TEAM_ROLES)[number];

/**
 * Enum-ish columns are plain `text`, not Postgres enums: the desktop board
 * stores free strings, and a Postgres enum would turn every new column or
 * actor kind into a migration that has to land before any client can use it.
 * The narrow `$type` plus the zod schemas at the edge are what keep the values
 * honest.
 */

export const users = pgTable(
  "users",
  {
    id: uuid("id").primaryKey().defaultRandom(),
    /** Clerk's user id — the only identity we trust to come from outside. */
    clerkUserId: text("clerk_user_id").notNull(),
    email: text("email"),
    name: text("name"),
    createdAt: bigint("created_at", { mode: "number" }).notNull(),
  },
  (table) => [
    uniqueIndex("users_clerk_user_id_key").on(table.clerkUserId),
    // Adding a member is a lookup by email; without this it is a scan.
    index("users_email_idx").on(table.email),
  ],
);

export const teams = pgTable("teams", {
  id: uuid("id").primaryKey().defaultRandom(),
  name: text("name").notNull(),
  createdBy: uuid("created_by")
    .notNull()
    .references(() => users.id),
  createdAt: bigint("created_at", { mode: "number" }).notNull(),
});

export const teamMembers = pgTable(
  "team_members",
  {
    teamId: uuid("team_id")
      .notNull()
      .references(() => teams.id, { onDelete: "cascade" }),
    userId: uuid("user_id")
      .notNull()
      .references(() => users.id, { onDelete: "cascade" }),
    role: text("role").$type<TeamRole>().notNull().default("member"),
    createdAt: bigint("created_at", { mode: "number" }).notNull(),
  },
  (table) => [
    // The composite key *is* the "one row per (team, user)" rule: a double
    // POST /members cannot produce two memberships to reconcile later.
    primaryKey({ columns: [table.teamId, table.userId] }),
    index("team_members_user_idx").on(table.userId),
  ],
);

export const boards = pgTable(
  "boards",
  {
    id: uuid("id").primaryKey().defaultRandom(),
    teamId: uuid("team_id")
      .notNull()
      .references(() => teams.id, { onDelete: "cascade" }),
    name: text("name").notNull(),
    /**
     * Bumped by every mutation. Clients poll `GET /boards/:id` with
     * `If-None-Match`, and this single number is what lets the server answer
     * 304 without reading a single task row.
     */
    rev: bigint("rev", { mode: "number" }).notNull().default(0),
    createdAt: bigint("created_at", { mode: "number" }).notNull(),
  },
  (table) => [index("boards_team_idx").on(table.teamId)],
);

export const boardTokens = pgTable(
  "board_tokens",
  {
    id: uuid("id").primaryKey().defaultRandom(),
    boardId: uuid("board_id")
      .notNull()
      .references(() => boards.id, { onDelete: "cascade" }),
    /** Human label for the machine holding it, e.g. "Simon's PC". */
    name: text("name").notNull(),
    /** SHA-256 hex of the full token. The plaintext is never stored. */
    tokenHash: text("token_hash").notNull(),
    createdBy: uuid("created_by")
      .notNull()
      .references(() => users.id),
    createdAt: bigint("created_at", { mode: "number" }).notNull(),
    /** Refreshed at most once a minute — see `shouldTouchToken`. */
    lastUsedAt: bigint("last_used_at", { mode: "number" }),
    /** Revocation is a timestamp, not a delete: the audit trail survives. */
    revokedAt: bigint("revoked_at", { mode: "number" }),
  },
  (table) => [
    // Every authenticated request is a lookup on this hash, so it has to be
    // the indexed unique column.
    uniqueIndex("board_tokens_token_hash_key").on(table.tokenHash),
    index("board_tokens_board_idx").on(table.boardId),
  ],
);

export const tasks = pgTable(
  "tasks",
  {
    /** Client-provided when the caller wants an id it already knows. */
    id: uuid("id").primaryKey().defaultRandom(),
    boardId: uuid("board_id")
      .notNull()
      .references(() => boards.id, { onDelete: "cascade" }),
    title: text("title").notNull(),
    description: text("description").notNull().default(""),
    column: text("column").$type<Column>().notNull().default("todo"),
    priority: integer("priority").notNull().default(1),
    /** Free-form label of the holder; null means free to pick up. */
    assignee: text("assignee"),
    assigneeKind: text("assignee_kind").$type<ActorKind>(),
    labels: jsonb("labels").$type<string[]>().notNull().default([]),
    dependsOn: jsonb("depends_on").$type<string[]>().notNull().default([]),
    order: doublePrecision("order").notNull().default(0),
    /** Optimistic concurrency: every write bumps it, PATCH may require it. */
    version: integer("version").notNull().default(0),
    createdAt: bigint("created_at", { mode: "number" }).notNull(),
    updatedAt: bigint("updated_at", { mode: "number" }).notNull(),
  },
  (table) => [
    index("tasks_board_idx").on(table.boardId),
    // `next_order` and `next-task` both scan one board's column.
    index("tasks_board_column_idx").on(table.boardId, table.column),
  ],
);

export const comments = pgTable(
  "comments",
  {
    id: uuid("id").primaryKey().defaultRandom(),
    taskId: uuid("task_id")
      .notNull()
      .references(() => tasks.id, { onDelete: "cascade" }),
    /** Free-form author label, e.g. "claude:pane-2" or "user". */
    author: text("author").notNull(),
    authorKind: text("author_kind").$type<ActorKind>(),
    /** Set when a signed-in human wrote it; agents leave it null. */
    authorUserId: uuid("author_user_id").references(() => users.id, {
      onDelete: "set null",
    }),
    text: text("text").notNull(),
    createdAt: bigint("created_at", { mode: "number" }).notNull(),
  },
  (table) => [index("comments_task_idx").on(table.taskId)],
);

/**
 * Append-only journal of what happened on a board. Agents coordinate through
 * the board itself; this is for the humans watching them.
 */
export const activity = pgTable(
  "activity",
  {
    id: uuid("id").primaryKey().defaultRandom(),
    boardId: uuid("board_id")
      .notNull()
      .references(() => boards.id, { onDelete: "cascade" }),
    /**
     * Not a foreign key: the journal outlives the task it talks about, and a
     * cascade delete would erase the record of the deletion itself.
     */
    taskId: uuid("task_id"),
    actor: text("actor").notNull(),
    actorKind: text("actor_kind").$type<ActorKind>(),
    action: text("action").notNull(),
    detail: jsonb("detail"),
    createdAt: bigint("created_at", { mode: "number" }).notNull(),
  },
  (table) => [
    // The feed is always "latest first for one board", cursored on created_at.
    index("activity_board_created_idx").on(table.boardId, table.createdAt.desc()),
  ],
);

export type UserRow = typeof users.$inferSelect;
export type TeamRow = typeof teams.$inferSelect;
export type BoardRow = typeof boards.$inferSelect;
export type BoardTokenRow = typeof boardTokens.$inferSelect;
export type TaskRow = typeof tasks.$inferSelect;
export type CommentRow = typeof comments.$inferSelect;
export type ActivityRow = typeof activity.$inferSelect;
