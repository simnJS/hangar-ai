import { beforeEach, describe, expect, it, vi } from "vitest";
import { clearPaneActivity, getPaneActivity, setPaneActivity, type AgentActivity } from "./agentState";
import type { ControlDeps } from "./control";
import type { ManagerJournalEntry } from "./managerEvents";
import type { BoardTask, JournalEntry } from "./orchestratorEvents";
import type { ProjectsDeps } from "./projects";
import { answerRemote, runRemote, type ChatPage, type ChatReader } from "./remoteControl";
import { answerInternal } from "./orchestratorBrief";
import type { ChatItem, RemoteChat, RemoteHistory, RemoteScreen, RemoteState } from "../remote/types";
import { DEFAULT_SETTINGS, type AppState, type Pane, type PaneRole, type Workspace } from "../types";

const journals = vi.hoisted(() => ({
  manager: [] as ManagerJournalEntry[],
  orchestrators: {} as Record<string, JournalEntry[]>,
}));

vi.mock("./managerEvents", async (importActual) => ({
  ...(await importActual<typeof import("./managerEvents")>()),
  managerJournal: () => journals.manager,
}));
vi.mock("./orchestratorEvents", async (importActual) => ({
  ...(await importActual<typeof import("./orchestratorEvents")>()),
  orchestratorJournal: (id: string) => journals.orchestrators[id] ?? [],
}));

function pane(id: string, name: string, role?: PaneRole, agent: Pane["agent"] = "claude"): Pane {
  return { id, name, agent, sessionId: null, cwd: null, shellId: null, title: null, ...(role ? { role } : {}) };
}

function workspace(id: string, name: string, panes: Pane[], extra: Partial<Workspace> = {}): Workspace {
  return {
    id,
    name,
    cwd: `C:\\dev\\${id}`,
    extraRoots: [],
    panes,
    themeId: null,
    shellId: null,
    tree: null,
    savedCommands: [],
    folderId: null,
    ...extra,
  };
}

function task(id: string, patch: Partial<BoardTask> = {}): BoardTask {
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
    created_at: 1,
    updated_at: 1,
    order: 1,
    ...patch,
  };
}

const act = (paneId: string, workspaceId: string, activity: AgentActivity) =>
  setPaneActivity(paneId, { workspaceId, activity, bridged: true });

interface Rig {
  deps: ControlDeps;
  typed: string[];
  opened: string[];
  screen: string[];
  /** Panes with a terminal; the others have none until their workspace opens. */
  live: Set<string>;
}

function rig(workspaces: Workspace[], board: BoardTask[] = [], withManager = true): Rig {
  const all = withManager
    ? [workspace("hq", "HQ", [pane("pm", "Manager", "manager")], { hq: true }), ...workspaces]
    : workspaces;
  const state = { ...DEFAULT_SETTINGS, workspaces: all, settings: { ...DEFAULT_SETTINGS } } as unknown as AppState;
  const typed: string[] = [];
  const opened: string[] = [];
  const screen = ["one", "two", "three"];
  const live = new Set(all.flatMap((ws) => ws.panes.map((p) => p.id)));
  const projects: ProjectsDeps = {
    snapshot: () => state,
    loadBoard: async () => board,
    loadMemory: async () => [],
    isRepo: async () => true,
    activity: getPaneActivity,
    asleep: (id) => id === "asleepWs",
    isOpen: (id) => !opened.length && id !== "asleepWs",
    journal: () => [],
  };
  const deps = {
    snapshot: () => state,
    enabled: () => false,
    openWorkspace: (id: string) => {
      opened.push(id);
      for (const ws of all) if (ws.id === id) for (const p of ws.panes) live.add(p.id);
    },
    isOpen: projects.isOpen,
    asleep: projects.asleep,
    touch: () => undefined,
    terminal: async (id: string) =>
      live.has(id)
        ? { paste: (t: string) => void typed.push(`paste:${id}:${t}`), read: (n: number) => screen.slice(-n), applicationCursor: false }
        : null,
    write: async (id: string, data: string) => void typed.push(`write:${id}:${JSON.stringify(data)}`),
    sleep: async () => undefined,
    activity: getPaneActivity,
    projects,
  } as unknown as ControlDeps;
  return { deps, typed, opened, screen, live };
}

const shop = () =>
  workspace("shop", "Shop", [pane("orch", "Jack", "orchestrator"), pane("w1", "Ben", "worker"), pane("sh", "term", undefined, "shell")]);

beforeEach(() => {
  journals.manager = [];
  journals.orchestrators = {};
  for (const id of ["orch", "w1", "sh", "pm", "a", "b"]) clearPaneActivity(id);
});

describe("remote_state", () => {
  it("lists projects without the HQ, with panes, board and manager", async () => {
    act("orch", "shop", "working");
    act("w1", "shop", "idle");
    act("pm", "hq", "yours");
    const { deps } = rig([shop()], [task("t1", { column: "doing" }), task("t2")]);
    const state = (await runRemote("remote_state", {}, deps)) as RemoteState;
    expect(state.manager).toEqual({ paneId: "pm", name: "Manager", activity: "yours" });
    expect(state.projects.map((p) => p.id)).toEqual(["shop"]);
    const [project] = state.projects;
    expect(project).toMatchObject({
      name: "Shop",
      git: true,
      asleep: false,
      orchestrator: { name: "Jack", activity: "working" },
      board: { todo: 1, doing: 1, review: 0, done: 0 },
      planStatus: null,
    });
    expect(project.panes).toEqual([
      { id: "orch", name: "Jack", agent: "claude", role: "orchestrator", activity: "working" },
      { id: "w1", name: "Ben", agent: "claude", role: "worker", activity: "idle" },
      { id: "sh", name: "term", agent: "shell", role: null, activity: null },
    ]);
    expect(typeof state.at).toBe("number");
  });

  it("has no manager when none is open", async () => {
    const { deps } = rig([shop()], [], false);
    const state = (await runRemote("remote_state", {}, deps)) as RemoteState;
    expect(state.manager).toBeNull();
  });

  it("lists what waits for the person, oldest first", async () => {
    act("w1", "shop", "waiting");
    journals.manager = [
      { at: 1000, event: { kind: "paneWaiting", workspaceId: "shop", workspace: "Shop", paneId: "w1", pane: "Ben", forMs: 0 } },
    ];
    const { deps } = rig([shop()], [
      task("n1", { labels: ["needs-you"] }),
      task("r1", { labels: ["ready-to-land"], column: "review" }),
      task("d1", { labels: ["needs-you"], column: "done" }),
    ]);
    const state = (await runRemote("remote_state", {}, deps)) as RemoteState;
    expect(state.waiting.map((w) => [w.kind, w.paneId ?? w.taskId])).toEqual([
      ["pane", "w1"],
      ["needsYou", "n1"],
      ["readyToLand", "r1"],
    ]);
    expect(state.waiting[0]).toMatchObject({ workspaceId: "shop", workspace: "Shop", pane: "Ben", since: 1000 });
    expect(state.waiting[1]).toMatchObject({ title: "Task n1", workspace: "Shop" });
    expect(state.projects[0].needsYou).toEqual([{ id: "n1", title: "Task n1" }]);
    expect(state.projects[0].readyToLand).toEqual([{ id: "r1", title: "Task r1" }]);
  });
});

describe("remote_screen", () => {
  it("returns the pane's text, found by name or id", async () => {
    act("w1", "shop", "working");
    const { deps } = rig([shop()]);
    const byName = (await runRemote("remote_screen", { pane: "ben", lines: 2 }, deps)) as RemoteScreen;
    expect(byName).toEqual({
      paneId: "w1",
      pane: "Ben",
      workspace: "Shop",
      agent: "claude",
      activity: "working",
      text: "two\nthree",
    });
    const byId = (await runRemote("remote_screen", { pane: "w1" }, deps)) as RemoteScreen;
    expect(byId.text).toBe("one\ntwo\nthree");
  });

  it("names an unknown pane", async () => {
    const { deps } = rig([shop()]);
    await expect(runRemote("remote_screen", { pane: "ghost" }, deps)).rejects.toThrow('No pane "ghost"');
  });
});

describe("remote_send", () => {
  it("types text then Enter, as the person", async () => {
    const { deps, typed } = rig([shop()]);
    expect(await runRemote("remote_send", { pane: "Ben", text: "hello" }, deps)).toEqual({ sent: true });
    expect(typed).toEqual(["paste:w1:hello", 'write:w1:"\\r"']);
  });

  it("sends keys, and text without submitting", async () => {
    const { deps, typed } = rig([shop()]);
    await runRemote("remote_send", { pane: "w1", text: "draft", submit: false, keys: ["up", "ctrl+c"] }, deps);
    expect(typed).toEqual(["paste:w1:draft", 'write:w1:"\\u001b[A"', 'write:w1:"\\u0003"']);
  });

  it("refuses a key outside the contract and sends nothing", async () => {
    const { deps, typed } = rig([shop()]);
    await expect(runRemote("remote_send", { pane: "w1", keys: ["backspace"] }, deps)).rejects.toThrow("Unknown key");
    expect(typed).toEqual([]);
  });

  it("opens a closed workspace first", async () => {
    const { deps, typed, opened, live } = rig([shop()]);
    live.delete("w1");
    await runRemote("remote_send", { pane: "w1", text: "wake" }, deps);
    expect(opened).toEqual(["shop"]);
    expect(typed[0]).toBe("paste:w1:wake");
  });

  it("names an unknown pane", async () => {
    const { deps } = rig([shop()]);
    await expect(runRemote("remote_send", { pane: "nobody", text: "x" }, deps)).rejects.toThrow('No pane "nobody"');
  });
});

describe("remote_permission", () => {
  it("answers Enter to allow and Escape to refuse a waiting pane", async () => {
    act("w1", "shop", "waiting");
    const { deps, typed } = rig([shop()]);
    expect(await runRemote("remote_permission", { pane: "Ben", allow: true }, deps)).toEqual({ answered: true });
    await runRemote("remote_permission", { pane: "Ben", allow: false }, deps);
    expect(typed).toEqual(['write:w1:"\\r"', 'write:w1:"\\u001b"']);
  });

  it("is refused unless the pane is waiting", async () => {
    act("w1", "shop", "working");
    const { deps, typed } = rig([shop()]);
    await expect(runRemote("remote_permission", { pane: "Ben", allow: true }, deps)).rejects.toThrow("not waiting");
    expect(typed).toEqual([]);
  });
});

describe("remote_manager", () => {
  it("types into the manager's pane", async () => {
    const { deps, typed } = rig([shop()]);
    expect(await runRemote("remote_manager", { text: "status?" }, deps)).toEqual({ sent: true });
    expect(typed).toEqual(["paste:pm:status?", 'write:pm:"\\r"']);
  });

  it("is refused without a manager", async () => {
    const { deps } = rig([shop()], [], false);
    await expect(runRemote("remote_manager", { text: "hi" }, deps)).rejects.toThrow(
      "Open the project manager on the PC",
    );
  });
});

describe("remote_history", () => {
  const stored = [
    { at: 1000, workspace: "Shop", kind: "landed", text: "Landed: Cart" },
    { at: 3000, workspace: "Shop", kind: "needsYou", text: "Needs you: Ship" },
    { at: 2000, workspace: "", kind: "appStopped", text: "" },
  ];
  const disk = (reads: number[] = []) => ({
    read: async (since: number) => {
      reads.push(since);
      return stored.filter((entry) => entry.at > since);
    },
  });

  it("answers from the log on disk, newest first, after `since`", async () => {
    const { deps } = rig([shop()]);
    const reads: number[] = [];
    const all = (await runRemote("remote_history", {}, deps, disk(reads))) as RemoteHistory;
    expect(all.entries.map((e) => [e.at, e.workspace, e.kind])).toEqual([
      [3000, "Shop", "needsYou"],
      [2000, "", "appStopped"],
      [1000, "Shop", "landed"],
    ]);
    const recent = (await runRemote("remote_history", { since: 1500 }, deps, disk(reads))) as RemoteHistory;
    expect(recent.entries.map((e) => e.at)).toEqual([3000, 2000]);
    expect(reads).toEqual([0, 1500]);
  });

  it("does not read the journals in memory, which a restart forgets", async () => {
    journals.manager = [
      { at: 9000, event: { kind: "readyToLand", workspaceId: "shop", workspace: "Shop", taskId: "t9", title: "Ship" } },
    ];
    const { deps } = rig([shop()]);
    const answer = (await runRemote("remote_history", {}, deps, { read: async () => [] })) as RemoteHistory;
    expect(answer.entries).toEqual([]);
  });

  it("words a line written without text in the language of the PC", async () => {
    const base = rig([shop()]).deps;
    const inLocale = (locale: "en" | "fr"): ControlDeps => ({
      ...base,
      snapshot: () => ({ ...base.snapshot(), settings: { ...base.snapshot().settings, locale } }),
    });
    const en = (await runRemote("remote_history", {}, inLocale("en"), disk())) as RemoteHistory;
    expect(en.entries.find((e) => e.kind === "appStopped")?.text).toBe("Hangar stopped");

    const french = inLocale("fr");
    const fr = (await runRemote("remote_history", {}, french, disk())) as RemoteHistory;
    expect(fr.entries.find((e) => e.kind === "appStopped")?.text).toBe("Hangar s'est arrêté");
    expect(fr.entries.find((e) => e.kind === "landed")?.text).toBe("Landed: Cart");
  });

  it("is an error when the log cannot be read", async () => {
    const { deps } = rig([shop()]);
    await expect(
      runRemote("remote_history", {}, deps, {
        read: async () => {
          throw new Error("disk");
        },
      }),
    ).rejects.toThrow("disk");
  });
});

describe("remote_chat", () => {
  const items: ChatItem[] = [
    { id: "u1", at: 1000, kind: "user", text: "Fix the cart", from: "person", images: 0 },
    { id: "t1", at: 2000, kind: "action", tool: "Bash", title: "Ran pnpm test", detail: "pnpm test", state: "running" },
  ];
  const page = (patch: Partial<ChatPage> = {}): ChatPage => ({
    items,
    older: true,
    cursor: "4096",
    question: null,
    openCall: null,
    ...patch,
  });
  const reader = (answer: ChatPage | null) => {
    const asked: Parameters<ChatReader>[0][] = [];
    const read: ChatReader = async (args) => {
      asked.push(args);
      return answer;
    };
    return { read, asked };
  };
  const chatShop = () =>
    workspace("shop", "Shop", [
      { ...pane("w1", "Ben", "worker"), sessionId: "sess-1", cwd: "C:\\wt\\ben" },
      { ...pane("orch", "Jack", "orchestrator"), sessionId: "sess-2" },
      { ...pane("new", "Ava", "worker") },
      { ...pane("sh", "term", undefined, "shell"), sessionId: "nope" },
    ]);
  const chat = (deps: ControlDeps, args: Record<string, unknown>, read: ChatReader) =>
    runRemote("remote_chat", args, deps, { read: async () => [] }, read) as Promise<RemoteChat>;

  it("reads a Claude Code pane's transcript, in its own folder, and passes the cursor on", async () => {
    act("w1", "shop", "working");
    const { deps } = rig([chatShop()]);
    const { read, asked } = reader(page());
    const answer = await chat(deps, { pane: "Ben", before: "9000", limit: 20 }, read);
    expect(asked).toEqual([{ sessionId: "sess-1", cwd: "C:\\wt\\ben", before: "9000", limit: 20 }]);
    expect(answer).toEqual({
      paneId: "w1",
      pane: "Ben",
      workspace: "Shop",
      agent: "claude",
      activity: "working",
      sessionId: "sess-1",
      items,
      older: true,
      cursor: "4096",
      pending: null,
    });
  });

  it("reads a pane without a folder of its own in its workspace's, newest page first", async () => {
    const { deps } = rig([chatShop()]);
    const { read, asked } = reader(page({ older: false, cursor: null }));
    const answer = await chat(deps, { pane: "orch" }, read);
    expect(asked).toEqual([{ sessionId: "sess-2", cwd: "C:\\dev\\shop" }]);
    expect(answer).toMatchObject({ older: false, cursor: null, activity: null });
  });

  it("answers no items and why for a shell, a pane without a session, a session not written yet", async () => {
    const { deps } = rig([chatShop()]);
    const { read, asked } = reader(null);
    const shell = await chat(deps, { pane: "term" }, read);
    expect(shell).toMatchObject({ agent: "shell", items: [], older: false, cursor: null, pending: null, reason: "notClaude" });
    const fresh = await chat(deps, { pane: "Ava" }, read);
    expect(fresh).toMatchObject({ sessionId: null, items: [], pending: null, reason: "noSession" });
    expect(asked).toEqual([]);
    const unwritten = await chat(deps, { pane: "Ben" }, read);
    expect(unwritten).toMatchObject({ sessionId: "sess-1", items: [], reason: "noTranscript" });
    expect(asked).toHaveLength(1);
  });

  it("derives a pending permission from the pane waiting on a call without its result", async () => {
    const openCall = { tool: "Bash", title: "Ran pnpm test", detail: "pnpm test" };
    const { deps } = rig([chatShop()]);
    act("w1", "shop", "waiting");
    const waiting = await chat(deps, { pane: "Ben" }, reader(page({ openCall })).read);
    expect(waiting.pending).toEqual({ kind: "permission", ...openCall });

    // Running on its own: the call is under way, nothing asks the person.
    act("w1", "shop", "working");
    const working = await chat(deps, { pane: "Ben" }, reader(page({ openCall })).read);
    expect(working.pending).toBeNull();

    // Waiting with every call answered: nothing this chat can name.
    act("w1", "shop", "waiting");
    const settled = await chat(deps, { pane: "Ben" }, reader(page()).read);
    expect(settled.pending).toBeNull();
  });

  it("puts an unanswered question first, whatever the activity", async () => {
    const question = {
      toolUseId: "toolu_q",
      questions: [
        {
          question: "Which currency?",
          header: "Currency",
          multiSelect: false,
          options: [{ label: "EUR", description: "Euro" }],
        },
      ],
    };
    const { deps } = rig([chatShop()]);
    act("w1", "shop", "waiting");
    const answer = await chat(
      deps,
      { pane: "Ben" },
      reader(page({ question, openCall: { tool: "Bash", title: "Ran ls" } })).read,
    );
    expect(answer.pending).toEqual({ kind: "question", ...question });
  });

  it("refuses a cursor that is not one and an unknown pane", async () => {
    const { deps } = rig([chatShop()]);
    const { read } = reader(page());
    await expect(chat(deps, { pane: "Ben", before: 12 }, read)).rejects.toThrow("cursor");
    await expect(chat(deps, { pane: "ghost" }, read)).rejects.toThrow('No pane "ghost"');
  });
});

describe("internal requests", () => {
  it("answers remote ops and leaves the others alone", async () => {
    const { deps } = rig([shop()]);
    const sent: unknown[][] = [];
    const send = async (...args: unknown[]) => void sent.push(args);
    expect(answerRemote({ id: "1", op: "orchestrator_briefing", args: {} }, deps, send)).toBe(false);
    expect(answerRemote({ id: "2", op: "remote_state", args: {} }, deps, send)).toBe(true);
    await vi.waitFor(() => expect(sent).toHaveLength(1));
    expect(sent[0].slice(0, 2)).toEqual(["2", true]);
  });

  it("answers an error for an unknown remote op", async () => {
    const { deps } = rig([shop()]);
    const sent: unknown[][] = [];
    answerRemote({ id: "3", op: "remote_nope", args: {} }, deps, async (...args) => void sent.push(args));
    await vi.waitFor(() => expect(sent).toHaveLength(1));
    expect(sent[0].slice(0, 2)).toEqual(["3", false]);
  });

  it("keeps the orchestrator briefing's listener to its own op", async () => {
    const sent: unknown[][] = [];
    const send = async (...args: unknown[]) => void sent.push(args);
    const briefing = { snapshot: () => ({ workspaces: [] }) } as never;
    expect(answerInternal({ id: "4", op: "remote_state", args: {} }, briefing, send)).toBe(false);
    expect(answerInternal({ id: "5", op: "orchestrator_briefing", args: { paneId: "x" } }, briefing, send)).toBe(true);
    await vi.waitFor(() => expect(sent.length).toBeGreaterThan(0));
    expect(sent.every((args) => args[0] === "5")).toBe(true);
  });
});
