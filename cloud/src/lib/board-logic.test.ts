import { describe, expect, it } from "vitest";

import type { Column } from "@/db/schema";
import {
  activityActionForPatch,
  claimIsPermitted,
  columnAfterClaim,
  hasUpdates,
  nextOrder,
  pickNextTask,
  taskPatchUpdates,
  type NextTaskCandidate,
} from "./board-logic";

describe("nextOrder", () => {
  it("puts the first card of a column at 1", () => {
    expect(nextOrder([])).toBe(1);
  });

  it("puts a new card below the lowest one", () => {
    expect(nextOrder([1, 2, 3])).toBe(4);
    expect(nextOrder([3, 1, 2])).toBe(4);
  });

  it("never goes below 1, like the fold from 0 in board.rs", () => {
    expect(nextOrder([-5, -2])).toBe(1);
  });

  it("handles the fractional orders a drag-and-drop reorder produces", () => {
    expect(nextOrder([1, 1.5, 2])).toBe(3);
  });
});

describe("claimIsPermitted", () => {
  it("lets anyone take a free task", () => {
    expect(claimIsPermitted(null, "claude-1")).toBe(true);
  });

  it("lets the holder re-claim its own task, like claim_task in board.rs", () => {
    // An agent whose call timed out retries; being told it lost the race to
    // itself would send it looking for other work it is already doing.
    expect(claimIsPermitted("claude-1", "claude-1")).toBe(true);
  });

  it("refuses anyone else — this is what stops duplicated work", () => {
    expect(claimIsPermitted("claude-1", "claude-2")).toBe(false);
  });

  it("is exact about the agent label", () => {
    expect(claimIsPermitted("claude-1", "claude-10")).toBe(false);
    expect(claimIsPermitted("claude-1", "Claude-1")).toBe(false);
    expect(claimIsPermitted("claude-1", "")).toBe(false);
  });
});

describe("columnAfterClaim", () => {
  it("starts a waiting task", () => {
    expect(columnAfterClaim("todo")).toBe("doing");
  });

  it("leaves a task claimed in review in review", () => {
    // Taking a card in review means reviewing it; moving it back to doing
    // would erase the fact that the work itself is finished.
    expect(columnAfterClaim("review")).toBe("review");
  });

  it("leaves every other column alone", () => {
    expect(columnAfterClaim("doing")).toBe("doing");
    expect(columnAfterClaim("done")).toBe("done");
  });
});

function candidate(overrides: Partial<NextTaskCandidate> = {}): NextTaskCandidate {
  return {
    id: "00000000-0000-4000-8000-000000000001",
    column: "todo",
    assignee: null,
    priority: 1,
    order: 1,
    dependsOn: [],
    createdAt: 1_000,
    ...overrides,
  };
}

describe("pickNextTask", () => {
  it("returns nothing on an empty board", () => {
    expect(pickNextTask([])).toBeNull();
  });

  it("takes the highest priority", () => {
    const low = candidate({ id: "a", priority: 1 });
    const high = candidate({ id: "b", priority: 5 });
    expect(pickNextTask([low, high])?.id).toBe("b");
  });

  it("breaks a priority tie with the topmost card", () => {
    const bottom = candidate({ id: "a", order: 9 });
    const top = candidate({ id: "b", order: 2 });
    expect(pickNextTask([bottom, top])?.id).toBe("b");
  });

  it("skips tasks somebody already holds", () => {
    const taken = candidate({ id: "a", priority: 9, assignee: "claude-1" });
    const free = candidate({ id: "b", priority: 1 });
    expect(pickNextTask([taken, free])?.id).toBe("b");
  });

  it("only looks at the todo column", () => {
    const doing = candidate({ id: "a", column: "doing", priority: 9 });
    const review = candidate({ id: "b", column: "review", priority: 9 });
    const todo = candidate({ id: "c", priority: 1 });
    expect(pickNextTask([doing, review, todo])?.id).toBe("c");
  });

  it("excludes a task whose dependency is not done", () => {
    const blocker = candidate({ id: "blocker", column: "doing" });
    const blocked = candidate({ id: "blocked", priority: 9, dependsOn: ["blocker"] });
    const free = candidate({ id: "free", priority: 1 });
    expect(pickNextTask([blocker, blocked, free])?.id).toBe("free");
  });

  it("hands out the blocked task once the dependency is done", () => {
    const blocker = candidate({ id: "blocker", column: "done" });
    const blocked = candidate({ id: "blocked", priority: 9, dependsOn: ["blocker"] });
    expect(pickNextTask([blocker, blocked])?.id).toBe("blocked");
  });

  it("requires every dependency, not just one", () => {
    const done = candidate({ id: "done-one", column: "done" });
    const pending = candidate({ id: "pending", column: "todo" });
    const blocked = candidate({
      id: "blocked",
      priority: 9,
      dependsOn: ["done-one", "pending"],
    });
    // `pending` is itself eligible, so the board is not stuck — but the task
    // waiting on it must not be handed out.
    expect(pickNextTask([done, pending, blocked])?.id).toBe("pending");
  });

  it("excludes a dependency on a task that is not on the board at all", () => {
    const orphan = candidate({ id: "orphan", dependsOn: ["ghost"] });
    expect(pickNextTask([orphan])).toBeNull();
  });

  it("returns nothing when everything is claimed or blocked", () => {
    const claimed = candidate({ id: "a", assignee: "claude-1" });
    const blocked = candidate({ id: "b", dependsOn: ["a"] });
    expect(pickNextTask([claimed, blocked])).toBeNull();
  });

  it("is deterministic when priority and order tie", () => {
    const older = candidate({ id: "b", createdAt: 10 });
    const newer = candidate({ id: "a", createdAt: 20 });
    expect(pickNextTask([newer, older])?.id).toBe("b");
    expect(pickNextTask([older, newer])?.id).toBe("b");
  });
});

describe("taskPatchUpdates", () => {
  it("only carries the fields the caller sent", () => {
    expect(taskPatchUpdates({ title: "new" }, "human")).toEqual({ title: "new" });
    expect(hasUpdates(taskPatchUpdates({}, "human"))).toBe(false);
  });

  it("stamps the assignee with the kind of caller that set it", () => {
    expect(taskPatchUpdates({ assignee: "claude-2" }, "agent")).toEqual({
      assignee: "claude-2",
      assigneeKind: "agent",
    });
  });

  it("lets release win over assignee, like patch_task in board.rs", () => {
    expect(taskPatchUpdates({ assignee: "claude-2", release: true }, "agent")).toEqual({
      assignee: null,
      assigneeKind: null,
    });
  });

  it("ignores release: false — handing back is opt-in", () => {
    expect(taskPatchUpdates({ assignee: "claude-2", release: false }, "agent")).toEqual({
      assignee: "claude-2",
      assigneeKind: "agent",
    });
  });

  it("keeps expected_version out of the field updates", () => {
    expect(taskPatchUpdates({ expected_version: 3 }, "human")).toEqual({});
  });

  it("passes an empty labels list through as a real change", () => {
    const updates = taskPatchUpdates({ labels: [] }, "human");
    expect(updates).toEqual({ labels: [] });
    expect(hasUpdates(updates)).toBe(true);
  });
});

describe("activityActionForPatch", () => {
  const from: Column = "todo";

  it("names a column change a move", () => {
    expect(activityActionForPatch({ column: "doing" }, from)).toBe("task.move");
  });

  it("does not call it a move when the column did not change", () => {
    expect(activityActionForPatch({ column: "todo" }, from)).toBe("task.update");
  });

  it("names a cleared assignee a release", () => {
    expect(activityActionForPatch({ assignee: null }, from)).toBe("task.release");
  });

  it("prefers the move when a patch both moves and releases", () => {
    expect(activityActionForPatch({ column: "review", assignee: null }, from)).toBe(
      "task.move",
    );
  });

  it("falls back to a plain update", () => {
    expect(activityActionForPatch({ title: "new" }, from)).toBe("task.update");
  });
});
