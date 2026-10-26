import { describe, expect, it } from "vitest";
import { taskForPane } from "./paneTask";
import type { Task } from "./board";
import type { Pane, Workspace } from "../types";

const pane = (id: string, name: string) => ({ id, name, agent: "claude" }) as Pane;
const workspaces = [
  { id: "ws", name: "Hangar", panes: [pane("p1", "Ava"), pane("p2", "Ben")] } as Workspace,
];

const task = (id: string, patch: Partial<Task>): Task => ({
  id,
  title: id,
  description: "",
  column: "doing",
  priority: 1,
  assignee: null,
  labels: [],
  comments: [],
  depends_on: [],
  created_at: 0,
  updated_at: 0,
  order: 0,
  ...patch,
});

describe("taskForPane", () => {
  it("finds the task an agent claimed under its pane's name", () => {
    const tasks = [task("t1", { assignee: "Ava (Hangar)" }), task("t2", { assignee: "Ben" })];
    expect(taskForPane(tasks, "p1", workspaces, "ws")?.id).toBe("t1");
    expect(taskForPane(tasks, "p2", workspaces, "ws")?.id).toBe("t2");
  });

  it("prefers work in progress over work in review, then the freshest", () => {
    const tasks = [
      task("review", { assignee: "Ava", column: "review", updated_at: 9 }),
      task("old", { assignee: "Ava", updated_at: 1 }),
      task("new", { assignee: "Ava", updated_at: 5 }),
    ];
    expect(taskForPane(tasks, "p1", workspaces, "ws")?.id).toBe("new");
  });

  it("ignores done, unassigned and unresolvable tasks", () => {
    const tasks = [
      task("done", { assignee: "Ava", column: "done" }),
      task("free", {}),
      task("stranger", { assignee: "someone-else" }),
    ];
    expect(taskForPane(tasks, "p1", workspaces, "ws")).toBeNull();
    expect(taskForPane(null, "p1", workspaces, "ws")).toBeNull();
  });
});
