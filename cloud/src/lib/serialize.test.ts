import { describe, expect, it } from "vitest";

import type { CommentRow, TaskRow } from "@/db/schema";
import { groupCommentsByTask, serializeComment, serializeTask } from "./serialize";

const TASK_ID = "3f1d9a4e-6b2c-4f0a-8f3e-1c2d3e4f5a6b";

function taskRow(overrides: Partial<TaskRow> = {}): TaskRow {
  return {
    id: TASK_ID,
    boardId: "b0000000-0000-4000-8000-000000000000",
    title: "Ship the cloud board",
    description: "",
    column: "todo",
    priority: 1,
    assignee: null,
    assigneeKind: null,
    labels: [],
    dependsOn: [],
    order: 1,
    version: 0,
    createdAt: 1_700_000_000_000,
    updatedAt: 1_700_000_000_000,
    ...overrides,
  };
}

function commentRow(overrides: Partial<CommentRow> = {}): CommentRow {
  return {
    id: "c0000000-0000-4000-8000-000000000000",
    taskId: TASK_ID,
    author: "claude-1",
    authorKind: "agent",
    authorUserId: null,
    text: "claimed and started",
    createdAt: 1_700_000_000_001,
    ...overrides,
  };
}

describe("serializeTask", () => {
  it("emits the snake_case shape the desktop board speaks", () => {
    expect(serializeTask(taskRow())).toEqual({
      id: TASK_ID,
      title: "Ship the cloud board",
      description: "",
      column: "todo",
      priority: 1,
      assignee: null,
      assignee_kind: null,
      labels: [],
      comments: [],
      depends_on: [],
      order: 1,
      version: 0,
      created_at: 1_700_000_000_000,
      updated_at: 1_700_000_000_000,
    });
  });

  it("never leaks the board id or other internals onto the wire", () => {
    const wire = serializeTask(taskRow());
    expect(wire).not.toHaveProperty("boardId");
    expect(wire).not.toHaveProperty("board_id");
  });

  it("embeds comments inside the task, like board.rs", () => {
    const wire = serializeTask(taskRow(), [commentRow()]);
    expect(wire.comments).toEqual([
      {
        id: "c0000000-0000-4000-8000-000000000000",
        author: "claude-1",
        author_kind: "agent",
        text: "claimed and started",
        created_at: 1_700_000_000_001,
      },
    ]);
  });
});

describe("serializeComment", () => {
  it("keeps the author's internal user id private", () => {
    const wire = serializeComment(
      commentRow({ authorUserId: "u0000000-0000-4000-8000-000000000000" }),
    );
    expect(wire).not.toHaveProperty("author_user_id");
    expect(wire).not.toHaveProperty("task_id");
  });
});

describe("groupCommentsByTask", () => {
  it("buckets comments per task and keeps their order", () => {
    const grouped = groupCommentsByTask([
      commentRow({ id: "1", text: "first" }),
      commentRow({ id: "2", text: "second" }),
      commentRow({ id: "3", taskId: "other", text: "elsewhere" }),
    ]);

    expect(grouped.get(TASK_ID)?.map((c) => c.text)).toEqual(["first", "second"]);
    expect(grouped.get("other")?.map((c) => c.text)).toEqual(["elsewhere"]);
  });

  it("returns nothing for a task with no comments", () => {
    expect(groupCommentsByTask([]).get(TASK_ID)).toBeUndefined();
  });
});
