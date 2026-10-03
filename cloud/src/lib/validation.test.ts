import { describe, expect, it } from "vitest";

import {
  activityQuerySchema,
  claimSchema,
  isUuid,
  newCommentSchema,
  newMemberSchema,
  newTaskSchema,
  newTokenSchema,
  taskPatchSchema,
} from "./validation";

const TASK_ID = "3f1d9a4e-6b2c-4f0a-8f3e-1c2d3e4f5a6b";
const OTHER_ID = "9a8b7c6d-5e4f-4a3b-9c8d-7e6f5a4b3c2d";

describe("newTaskSchema", () => {
  it("fills in the defaults an agent leaves out", () => {
    const parsed = newTaskSchema.parse({ title: "Ship the cloud board" });
    expect(parsed).toEqual({
      title: "Ship the cloud board",
      description: "",
      column: "todo",
      priority: 1,
      labels: [],
      depends_on: [],
    });
  });

  it("accepts a full payload, client-provided id included", () => {
    const parsed = newTaskSchema.parse({
      id: TASK_ID,
      title: "Wire the dashboard",
      description: "React, later",
      column: "doing",
      priority: 5,
      labels: ["ui"],
      depends_on: [OTHER_ID],
    });
    expect(parsed.id).toBe(TASK_ID);
    expect(parsed.depends_on).toEqual([OTHER_ID]);
  });

  it("refuses a column that is not one of the four", () => {
    expect(newTaskSchema.safeParse({ title: "x", column: "backlog" }).success).toBe(
      false,
    );
    expect(newTaskSchema.safeParse({ title: "x", column: "" }).success).toBe(false);
    expect(newTaskSchema.safeParse({ title: "x", column: "TODO" }).success).toBe(false);
  });

  it("refuses an empty or whitespace-only title", () => {
    expect(newTaskSchema.safeParse({}).success).toBe(false);
    expect(newTaskSchema.safeParse({ title: "" }).success).toBe(false);
    expect(newTaskSchema.safeParse({ title: "   " }).success).toBe(false);
  });

  it("keeps priority inside the u8 range board.rs stores", () => {
    expect(newTaskSchema.safeParse({ title: "x", priority: 255 }).success).toBe(true);
    expect(newTaskSchema.safeParse({ title: "x", priority: 256 }).success).toBe(false);
    expect(newTaskSchema.safeParse({ title: "x", priority: -1 }).success).toBe(false);
    expect(newTaskSchema.safeParse({ title: "x", priority: 1.5 }).success).toBe(false);
  });

  it("refuses a dependency that is not a task id", () => {
    expect(
      newTaskSchema.safeParse({ title: "x", depends_on: ["not-a-uuid"] }).success,
    ).toBe(false);
  });

  it("drops unknown keys instead of failing, the way serde does", () => {
    const parsed = newTaskSchema.parse({ title: "x", assignee: "claude-1" });
    expect(parsed).not.toHaveProperty("assignee");
  });
});

describe("taskPatchSchema", () => {
  it("accepts an empty patch — the handler decides it is a no-op", () => {
    expect(taskPatchSchema.parse({})).toEqual({});
  });

  it("keeps release and assignee apart so the handler can arbitrate", () => {
    const parsed = taskPatchSchema.parse({ assignee: "claude-1", release: true });
    expect(parsed).toEqual({ assignee: "claude-1", release: true });
  });

  it("takes an expected_version for optimistic concurrency", () => {
    expect(taskPatchSchema.parse({ title: "x", expected_version: 4 }).expected_version).toBe(4);
    expect(taskPatchSchema.safeParse({ expected_version: -1 }).success).toBe(false);
    expect(taskPatchSchema.safeParse({ expected_version: 1.5 }).success).toBe(false);
  });

  it("refuses an unknown column here too", () => {
    expect(taskPatchSchema.safeParse({ column: "archived" }).success).toBe(false);
  });

  it("takes fractional orders and rejects non-finite ones", () => {
    expect(taskPatchSchema.parse({ order: 2.5 }).order).toBe(2.5);
    expect(taskPatchSchema.safeParse({ order: Number.POSITIVE_INFINITY }).success).toBe(
      false,
    );
    expect(taskPatchSchema.safeParse({ order: "2" }).success).toBe(false);
  });
});

describe("claimSchema", () => {
  it("defaults the claimant to an agent", () => {
    expect(claimSchema.parse({ agent: "claude-1" })).toEqual({
      agent: "claude-1",
      kind: "agent",
    });
  });

  it("lets a person claim from the dashboard", () => {
    expect(claimSchema.parse({ agent: "simon", kind: "human" }).kind).toBe("human");
  });

  it("refuses a nameless claim — the assignee is what other agents read", () => {
    expect(claimSchema.safeParse({}).success).toBe(false);
    expect(claimSchema.safeParse({ agent: "" }).success).toBe(false);
  });

  it("refuses an actor kind that is neither human nor agent", () => {
    expect(claimSchema.safeParse({ agent: "x", kind: "robot" }).success).toBe(false);
  });
});

describe("newCommentSchema", () => {
  it("requires an author and a body", () => {
    expect(newCommentSchema.parse({ author: "claude-1", text: "done" })).toEqual({
      author: "claude-1",
      text: "done",
    });
    expect(newCommentSchema.safeParse({ author: "claude-1" }).success).toBe(false);
    expect(newCommentSchema.safeParse({ author: "claude-1", text: " " }).success).toBe(
      false,
    );
  });
});

describe("activityQuerySchema", () => {
  it("defaults to a page of 50", () => {
    expect(activityQuerySchema.parse({})).toEqual({ limit: 50 });
  });

  it("reads the cursor and limit out of query strings", () => {
    expect(activityQuerySchema.parse({ limit: "10", before: "1700000000000" })).toEqual({
      limit: 10,
      before: 1_700_000_000_000,
    });
  });

  it("caps the page size and refuses nonsense", () => {
    expect(activityQuerySchema.safeParse({ limit: "500" }).success).toBe(false);
    expect(activityQuerySchema.safeParse({ limit: "0" }).success).toBe(false);
    expect(activityQuerySchema.safeParse({ limit: "abc" }).success).toBe(false);
  });
});

describe("account payloads", () => {
  it("requires a real email to add a member", () => {
    expect(newMemberSchema.parse({ email: "simon@example.com" }).email).toBe(
      "simon@example.com",
    );
    expect(newMemberSchema.safeParse({ email: "simon" }).success).toBe(false);
  });

  it("requires a label on a token so a revocation list is readable", () => {
    expect(newTokenSchema.parse({ name: "Simon's PC" }).name).toBe("Simon's PC");
    expect(newTokenSchema.safeParse({ name: "" }).success).toBe(false);
  });
});

describe("isUuid", () => {
  it("guards path segments before they reach the database", () => {
    expect(isUuid(TASK_ID)).toBe(true);
    expect(isUuid("not-a-uuid")).toBe(false);
    expect(isUuid("")).toBe(false);
    expect(isUuid("'; drop table tasks; --")).toBe(false);
  });
});
