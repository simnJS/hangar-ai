/**
 * The shared task board, in memory.
 *
 * The real one is a JSON file per workspace behind a lock in Rust, reached
 * both by the UI and by the agents over MCP. Here the UI and the fake agents
 * both call straight into this module, and both raise `board:changed`, which
 * is what makes the board view update while you watch a pane work.
 */
import { dispatch } from "../mock/event";
import type { BoardColumn, NewTask, Task, TaskPatch } from "../../src/lib/board";

let serial = 100;
const now = () => Date.now();

function task(
  id: string,
  title: string,
  column: BoardColumn,
  extra: Partial<Task> = {},
): Task {
  return {
    id,
    title,
    description: "",
    column,
    priority: 1,
    assignee: null,
    labels: [],
    comments: [],
    depends_on: [],
    created_at: now() - 3600_000,
    updated_at: now() - 3600_000,
    order: serial++,
    ...extra,
  };
}

const boards = new Map<string, Task[]>();

function seed(cwd: string): Task[] {
  if (cwd.endsWith("storefront")) {
    return [
      task("21", "Cart total ignores the currency", "todo", { priority: 2, labels: ["bug"] }),
      task("22", "Checkout: keep the address on a failed payment", "todo"),
      task("23", "Product grid: skeleton while images load", "doing", { assignee: "claude" }),
    ];
  }
  return [
    task("09", "Refactor the pty bridge for WSL paths", "todo", {
      priority: 2,
      labels: ["rust"],
      description: "wslpath -w on the way in, so a Windows path opens in the right place.",
    }),
    task("11", "Tests for atomic board claims", "todo", {
      labels: ["rust", "tests"],
      description: "Two agents racing for one task: exactly one wins.",
    }),
    task("12", "Rich Presence badge art for opencode", "todo", { priority: 0 }),
    task("14", "Port the installer bitmaps to the new mark", "todo"),
    task("06", "Build every platform in one job", "review", { assignee: "codex" }),
    task("07", "Shift+Enter sends a newline", "done", { assignee: "claude" }),
    task("05", "Keep Shift+Enter submitting in a plain shell", "done", { assignee: "claude" }),
  ];
}

function boardFor(cwd: string): Task[] {
  let list = boards.get(cwd);
  if (!list) {
    list = seed(cwd);
    boards.set(cwd, list);
  }
  return list;
}

/** Same event the Rust side raises, so BoardView refreshes on its own. */
function changed(cwd: string) {
  dispatch("board:changed", cwd);
}

export function load(cwd: string): { tasks: Task[] } {
  return { tasks: boardFor(cwd).map((t) => ({ ...t })) };
}

export function create(cwd: string, input: NewTask): Task {
  const list = boardFor(cwd);
  const created = task(String(++serial), input.title, input.column ?? "todo", {
    description: input.description ?? "",
    priority: input.priority ?? 1,
    labels: input.labels ?? [],
    depends_on: input.depends_on ?? [],
    created_at: now(),
    updated_at: now(),
  });
  list.unshift(created);
  changed(cwd);
  return { ...created };
}

export function update(cwd: string, id: string, patch: TaskPatch): Task {
  const found = boardFor(cwd).find((t) => t.id === id);
  if (!found) throw new Error(`no task ${id}`);
  if (patch.release) found.assignee = null;
  if (patch.assignee !== undefined) found.assignee = patch.assignee;
  if (patch.title !== undefined) found.title = patch.title;
  if (patch.description !== undefined) found.description = patch.description;
  if (patch.column !== undefined) found.column = patch.column;
  if (patch.priority !== undefined) found.priority = patch.priority;
  if (patch.labels !== undefined) found.labels = patch.labels;
  if (patch.depends_on !== undefined) found.depends_on = patch.depends_on;
  if (patch.order !== undefined) found.order = patch.order;
  found.updated_at = now();
  changed(cwd);
  return { ...found };
}

export function remove(cwd: string, id: string): void {
  const list = boardFor(cwd);
  const at = list.findIndex((t) => t.id === id);
  if (at >= 0) list.splice(at, 1);
  changed(cwd);
}

/** Fails when someone already holds it, which is the point of the real lock. */
export function claim(cwd: string, id: string, agent: string): Task {
  const found = boardFor(cwd).find((t) => t.id === id);
  if (!found) throw new Error(`no task ${id}`);
  if (found.assignee && found.assignee !== agent) {
    throw new Error(`task ${id} is already held by ${found.assignee}`);
  }
  found.assignee = agent;
  found.column = "doing";
  found.updated_at = now();
  changed(cwd);
  return { ...found };
}

export function comment(cwd: string, id: string, author: string, text: string): Task {
  const found = boardFor(cwd).find((t) => t.id === id);
  if (!found) throw new Error(`no task ${id}`);
  found.comments.push({ id: String(++serial), author, text, created_at: now() });
  found.updated_at = now();
  changed(cwd);
  return { ...found };
}

/** Highest priority free task whose dependencies are all done. */
export function next(cwd: string): Task | null {
  const list = boardFor(cwd);
  const done = new Set(list.filter((t) => t.column === "done").map((t) => t.id));
  const free = list
    .filter((t) => t.column === "todo" && !t.assignee)
    .filter((t) => t.depends_on.every((d) => done.has(d)))
    .sort((a, b) => b.priority - a.priority || a.order - b.order);
  return free[0] ? { ...free[0] } : null;
}

export function counts(cwd: string) {
  const list = boardFor(cwd);
  const by = (c: BoardColumn) => list.filter((t) => t.column === c).length;
  return { todo: by("todo"), doing: by("doing"), review: by("review"), done: by("done") };
}
