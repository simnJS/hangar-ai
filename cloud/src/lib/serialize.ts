import type {
  ActivityRow,
  BoardRow,
  BoardTokenRow,
  CommentRow,
  TaskRow,
  TeamRow,
  UserRow,
} from "@/db/schema";

/**
 * Every wire shape lives here.
 *
 * The board payloads are snake_case with embedded comments — field for field
 * what the desktop board emits (`src-tauri/src/board.rs`), so a client can
 * point at either backend without a translation layer. `version` and
 * `assignee_kind` are the only additions; the local board ignores unknown
 * fields, so they cost nothing there.
 */

export function serializeUser(row: UserRow) {
  return {
    id: row.id,
    email: row.email,
    name: row.name,
    created_at: row.createdAt,
  };
}

export function serializeTeam(row: TeamRow, role?: string) {
  return {
    id: row.id,
    name: row.name,
    created_by: row.createdBy,
    created_at: row.createdAt,
    ...(role ? { role } : {}),
  };
}

export function serializeBoard(row: BoardRow) {
  return {
    id: row.id,
    team_id: row.teamId,
    name: row.name,
    rev: row.rev,
    created_at: row.createdAt,
  };
}

/** Token metadata only — the plaintext exists once, in the create response. */
export function serializeToken(row: BoardTokenRow) {
  return {
    id: row.id,
    board_id: row.boardId,
    name: row.name,
    created_by: row.createdBy,
    created_at: row.createdAt,
    last_used_at: row.lastUsedAt,
    revoked_at: row.revokedAt,
  };
}

export function serializeComment(row: CommentRow) {
  return {
    id: row.id,
    author: row.author,
    author_kind: row.authorKind,
    text: row.text,
    created_at: row.createdAt,
  };
}

export function serializeTask(row: TaskRow, taskComments: readonly CommentRow[] = []) {
  return {
    id: row.id,
    title: row.title,
    description: row.description,
    column: row.column,
    priority: row.priority,
    assignee: row.assignee,
    assignee_kind: row.assigneeKind,
    labels: row.labels,
    comments: taskComments.map(serializeComment),
    depends_on: row.dependsOn,
    order: row.order,
    version: row.version,
    created_at: row.createdAt,
    updated_at: row.updatedAt,
  };
}

/** Groups comments by task so a whole board is serialized in one pass. */
export function groupCommentsByTask(
  rows: readonly CommentRow[],
): Map<string, CommentRow[]> {
  const grouped = new Map<string, CommentRow[]>();
  for (const row of rows) {
    const bucket = grouped.get(row.taskId);
    if (bucket) bucket.push(row);
    else grouped.set(row.taskId, [row]);
  }
  return grouped;
}

export function serializeActivity(row: ActivityRow) {
  return {
    id: row.id,
    task_id: row.taskId,
    actor: row.actor,
    actor_kind: row.actorKind,
    action: row.action,
    detail: row.detail,
    created_at: row.createdAt,
  };
}
