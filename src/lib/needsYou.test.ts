import { describe, expect, it, vi } from "vitest";
import type { Task } from "./board";
import type { AgentId, Pane, Workspace } from "../types";

// The hooks reach for the board's loader and the notifier, which talk to
// Tauri; the list and the notices' arithmetic need neither.
vi.mock("./paneTask", () => ({ useBoardsOf: () => new Map() }));
vi.mock("./notify", () => ({ notify: () => Promise.resolve() }));

const {
  EMPTY_MEMORY,
  latestNote,
  needsYouItems,
  noticeDelta,
  watchedBoards,
  NEEDS_YOU_LABEL,
  READY_TO_LAND_LABEL,
} = await import("./needsYou");

function pane(id: string, agent: AgentId = "claude", patch: Partial<Pane> = {}): Pane {
  return { id, name: id.toUpperCase(), agent, sessionId: null, cwd: null, shellId: null, title: null, ...patch };
}

function workspace(id: string, panes: Pane[], cwd = `C:\\dev\\${id}`): Workspace {
  return {
    id,
    name: id,
    cwd,
    extraRoots: [],
    panes,
    themeId: null,
    shellId: null,
    tree: null,
    savedCommands: [],
    folderId: null,
  };
}

let order = 0;
function task(id: string, patch: Partial<Task> = {}): Task {
  return {
    id,
    title: `Task ${id}`,
    description: "",
    column: "todo",
    priority: 1,
    assignee: null,
    labels: [],
    comments: [],
    depends_on: [],
    created_at: 1_000,
    updated_at: 1_000,
    order: order++,
    ...patch,
  };
}

const orchestrator = (id: string) => pane(id, "claude", { role: "orchestrator" });
const worker = (id: string, taskId: string) => pane(id, "claude", { role: "worker", taskId });

/** One orchestrated workspace and one that is not, as most machines would be. */
function world(tasks: Task[], extra: Partial<{ waiting: string[] }> = {}) {
  const orchestrated = workspace("checkout", [orchestrator("iris"), worker("jade", "31"), pane("dan", "shell")]);
  const plain = workspace("site", [pane("leo"), pane("eve")]);
  const workspaces = [orchestrated, plain];
  return {
    workspaces,
    waiting: extra.waiting ?? [],
    boards: new Map<string, Task[] | null>([[orchestrated.cwd, tasks], [plain.cwd, []]]),
  };
}

const kinds = (items: { kind: string }[]) => items.map((item) => item.kind);

describe("needsYouItems", () => {
  it("is empty for anyone who never pinned an orchestrator, whatever waits", () => {
    const plain = workspace("site", [pane("leo"), pane("eve")]);
    const tasks = [task("1", { labels: [NEEDS_YOU_LABEL] })];
    expect(
      needsYouItems({
        workspaces: [plain],
        waiting: ["leo"],
        boards: new Map([[plain.cwd, tasks]]),
      }),
    ).toEqual([]);
  });

  it("lists agents waiting for an answer in every workspace once one orchestrates", () => {
    const items = needsYouItems(world([], { waiting: ["eve", "jade"] }));
    expect(items.map((item) => item.kind === "waiting" && item.paneName)).toEqual(["JADE", "EVE"]);
    expect(items[1]).toMatchObject({ workspaceId: "site", workspaceName: "site", paneId: "eve" });
  });

  it("lists the questions and the deliveries of the orchestrated boards, nothing else", () => {
    const items = needsYouItems(
      world([
        task("30", { labels: [NEEDS_YOU_LABEL] }),
        task("33", { column: "review", labels: [READY_TO_LAND_LABEL] }),
        task("31", { column: "doing", labels: ["bug"] }),
      ]),
    );
    expect(kinds(items)).toEqual(["question", "land"]);
    expect(items[0]).toMatchObject({ kind: "question", orchestratorId: "iris", cwd: "C:\\dev\\checkout" });
    expect(items.map((item) => item.kind !== "waiting" && item.task.id)).toEqual(["30", "33"]);
  });

  it("puts what blocks an agent first, then the questions, then the merges", () => {
    const items = needsYouItems(
      world(
        [
          task("33", { column: "review", labels: [READY_TO_LAND_LABEL] }),
          task("30", { labels: [NEEDS_YOU_LABEL] }),
        ],
        { waiting: ["eve"] },
      ),
    );
    expect(kinds(items)).toEqual(["waiting", "question", "land"]);
  });

  it("asks the most urgent question first, and lands the oldest delivery first", () => {
    const items = needsYouItems(
      world([
        task("a", { labels: [NEEDS_YOU_LABEL], priority: 1, updated_at: 5 }),
        task("b", { labels: [NEEDS_YOU_LABEL], priority: 3, updated_at: 1 }),
        task("c", { labels: [NEEDS_YOU_LABEL], priority: 1, updated_at: 9 }),
        task("new", { column: "review", labels: [READY_TO_LAND_LABEL], updated_at: 50 }),
        task("old", { column: "review", labels: [READY_TO_LAND_LABEL], updated_at: 10 }),
      ]),
    );
    expect(items.map((item) => item.kind !== "waiting" && item.task.id)).toEqual([
      "b",
      "c",
      "a",
      "old",
      "new",
    ]);
  });

  it("leaves done tasks out, whatever labels they kept", () => {
    const items = needsYouItems(
      world([
        task("1", { column: "done", labels: [NEEDS_YOU_LABEL] }),
        task("2", { column: "done", labels: [READY_TO_LAND_LABEL] }),
      ]),
    );
    expect(items).toEqual([]);
  });

  it("lists a task labelled both ways once, as the question it still is", () => {
    const items = needsYouItems(
      world([task("1", { column: "review", labels: [READY_TO_LAND_LABEL, NEEDS_YOU_LABEL] })]),
    );
    expect(kinds(items)).toEqual(["question"]);
  });

  it("waits for a board that is still being read rather than guessing", () => {
    const { workspaces } = world([]);
    const items = needsYouItems({ workspaces, waiting: [], boards: new Map() });
    expect(items).toEqual([]);
  });

  it("reads a board two workspaces share once, under the first one", () => {
    const a = workspace("a", [orchestrator("o1")], "C:\\dev\\shared");
    const b = workspace("b", [worker("w1", "1")], "C:\\dev\\shared");
    const items = needsYouItems({
      workspaces: [a, b],
      waiting: [],
      boards: new Map([["C:\\dev\\shared", [task("1", { labels: [NEEDS_YOU_LABEL] })]]]),
    });
    expect(items.map((item) => item.workspaceId)).toEqual(["a"]);
  });

  it("names no orchestrator to answer when a worker is all that is left", () => {
    const left = workspace("left", [worker("w", "1")]);
    const [item] = needsYouItems({
      workspaces: [left],
      waiting: [],
      boards: new Map([[left.cwd, [task("1", { labels: [NEEDS_YOU_LABEL] })]]]),
    });
    expect(item).toMatchObject({ kind: "question", orchestratorId: null });
  });

  it("keys each item by what it stands for, the same from one read to the next", () => {
    const tasks = [task("30", { labels: [NEEDS_YOU_LABEL] })];
    const first = needsYouItems(world(tasks, { waiting: ["eve"] }));
    const again = needsYouItems(world(tasks.map((entry) => ({ ...entry })), { waiting: ["eve"] }));
    expect(again.map((item) => item.key)).toEqual(first.map((item) => item.key));
    expect(new Set(first.map((item) => item.key)).size).toBe(first.length);
  });
});

describe("watchedBoards", () => {
  it("reads only the boards of the workspaces that orchestrate, once each", () => {
    const a = workspace("a", [orchestrator("o1")], "C:\\dev\\shared");
    const b = workspace("b", [worker("w1", "1")], "C:\\dev\\shared");
    const c = workspace("c", [pane("x")]);
    const d = workspace("d", [orchestrator("o2")]);
    expect(watchedBoards([a, b, c, d])).toEqual(["C:\\dev\\shared", "C:\\dev\\d"]);
  });
});

describe("latestNote", () => {
  it("says what the last comment says, on one line", () => {
    const comments = [
      { id: "1", author: "Iris", text: "First thought", created_at: 1 },
      { id: "2", author: "Iris", text: "\n  Stripe or Adyen?\nDetails follow.", created_at: 2 },
    ];
    expect(latestNote({ comments })).toBe("Stripe or Adyen?");
    expect(latestNote({ comments: [] })).toBeNull();
  });
});

describe("noticeDelta", () => {
  const question = (id: string) => task(id, { labels: [NEEDS_YOU_LABEL] });
  const cwd = "C:\\dev\\checkout";

  it("says nothing about what was already waiting when a board is first read", () => {
    const items = needsYouItems(world([question("1")]));
    const { fresh, memory } = noticeDelta(EMPTY_MEMORY, items, new Set([cwd]));
    expect(fresh).toEqual([]);
    expect([...memory.primed]).toEqual([cwd]);
  });

  it("tells of a question or a delivery that appears once the board is known", () => {
    const loaded = new Set([cwd]);
    let state = noticeDelta(EMPTY_MEMORY, needsYouItems(world([])), loaded).memory;
    const next = noticeDelta(
      state,
      needsYouItems(world([question("1"), task("2", { column: "review", labels: [READY_TO_LAND_LABEL] })])),
      loaded,
    );
    expect(next.fresh.map((item) => item.kind)).toEqual(["question", "land"]);
    state = next.memory;
    // Read again, unchanged: no news.
    expect(noticeDelta(state, needsYouItems(world([question("1"), task("2", { column: "review", labels: [READY_TO_LAND_LABEL] })])), loaded).fresh).toEqual([]);
  });

  it("tells again of an item that left and came back", () => {
    const loaded = new Set([cwd]);
    let memory = noticeDelta(EMPTY_MEMORY, needsYouItems(world([])), loaded).memory;
    memory = noticeDelta(memory, needsYouItems(world([question("1")])), loaded).memory;
    memory = noticeDelta(memory, needsYouItems(world([])), loaded).memory;
    expect(noticeDelta(memory, needsYouItems(world([question("1")])), loaded).fresh).toHaveLength(1);
  });

  it("leaves the agents waiting in a dialog to their own pane's notification", () => {
    const loaded = new Set([cwd]);
    const memory = noticeDelta(EMPTY_MEMORY, needsYouItems(world([])), loaded).memory;
    const { fresh } = noticeDelta(memory, needsYouItems(world([], { waiting: ["eve"] })), loaded);
    expect(fresh).toEqual([]);
  });
});
