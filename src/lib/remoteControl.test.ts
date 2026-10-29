import { beforeEach, describe, expect, it, vi } from "vitest";
import { clearPaneActivity, getPaneActivity, setPaneActivity, type AgentActivity } from "./agentState";
import type { ControlDeps } from "./control";
import type { ManagerJournalEntry } from "./managerEvents";
import type { BoardTask, JournalEntry } from "./orchestratorEvents";
import type { ProjectsDeps } from "./projects";
import {
  answerRemote,
  noteBridgeEvent,
  runRemote,
  statsSources,
  usageReader,
  type ChatPage,
  type ChatReader,
  type StatsSources,
} from "./remoteControl";
import { answerInternal } from "./orchestratorBrief";
import type { StatsBucket, UsageStats } from "./stats";
import { addDays, modelLabel, startOfDay } from "./statsModel";
import type { ChatItem, RemoteChat, RemoteHistory, RemoteScreen, RemoteState, RemoteStats } from "../remote/types";
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
  /** What went to the hangar-bridge queue, as `pane:text`. */
  queued: string[];
  /** Workspaces asleep; opening one wakes it, as the window does. */
  asleep: Set<string>;
  /** Workspaces used, as `touch` counts them. */
  touched: string[];
}

function rig(workspaces: Workspace[], board: BoardTask[] = [], withManager = true): Rig {
  const all = withManager
    ? [workspace("hq", "HQ", [pane("pm", "Manager", "manager")], { hq: true }), ...workspaces]
    : workspaces;
  const state = { ...DEFAULT_SETTINGS, workspaces: all, settings: { ...DEFAULT_SETTINGS } } as unknown as AppState;
  const typed: string[] = [];
  const opened: string[] = [];
  const queued: string[] = [];
  const touched: string[] = [];
  const screen = ["one", "two", "three"];
  const live = new Set(all.flatMap((ws) => ws.panes.map((p) => p.id)));
  const asleep = new Set(["asleepWs"]);
  const open = new Set(all.map((ws) => ws.id).filter((id) => !asleep.has(id)));
  const projects: ProjectsDeps = {
    snapshot: () => state,
    loadBoard: async () => board,
    loadMemory: async () => [],
    isRepo: async () => true,
    activity: getPaneActivity,
    asleep: (id) => asleep.has(id),
    isOpen: (id) => open.has(id),
    journal: () => [],
  };
  const deps = {
    snapshot: () => state,
    enabled: () => false,
    // Its terminals mount at once: the shells start, the agents come later.
    openWorkspace: (id: string) => {
      opened.push(id);
      open.add(id);
      asleep.delete(id);
      for (const ws of all) if (ws.id === id) for (const p of ws.panes) live.add(p.id);
    },
    isOpen: projects.isOpen,
    asleep: projects.asleep,
    touch: (id: string) => void touched.push(id),
    terminal: async (id: string) =>
      live.has(id)
        ? { paste: (t: string) => void typed.push(`paste:${id}:${t}`), read: (n: number) => screen.slice(-n), applicationCursor: false }
        : null,
    write: async (id: string, data: string) => void typed.push(`write:${id}:${JSON.stringify(data)}`),
    alive: async (id: string) => live.has(id),
    enqueue: async (id: string, text: string, from?: string) => void queued.push(from ? `${id}:${text} (from ${from})` : `${id}:${text}`),
    sleep: async () => undefined,
    activity: getPaneActivity,
    projects,
  } as unknown as ControlDeps;
  return { deps, typed, opened, screen, live, queued, asleep, touched };
}

const shop = () =>
  workspace("shop", "Shop", [pane("orch", "Jack", "orchestrator"), pane("w1", "Ben", "worker"), pane("sh", "term", undefined, "shell")]);

beforeEach(() => {
  journals.manager = [];
  journals.orchestrators = {};
  for (const id of ["orch", "w1", "sh", "pm", "a", "b", "noe", "logs"]) {
    clearPaneActivity(id);
    noteBridgeEvent(id, { kind: "end", reason: "test" });
  }
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

  it("opens a closed workspace first, and types into a shell once it runs", async () => {
    const { deps, typed, opened, live } = rig([shop()]);
    live.delete("sh");
    await runRemote("remote_send", { pane: "term", text: "ls" }, deps);
    expect(opened).toEqual(["shop"]);
    expect(typed).toEqual(["paste:sh:ls", 'write:sh:"\\r"']);
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

  describe("what the pane's mod reports before the transcript has it", () => {
    const COLOR = {
      question: "Which color?",
      header: "Color",
      multiSelect: false,
      options: [
        { label: "Red", description: "Warm" },
        { label: "Blue", description: "Cold" },
      ],
    };
    const started = (toolUseId: string, tool: string, input: Record<string, unknown>, at = 5000) =>
      noteBridgeEvent("w1", { kind: "tool", phase: "start", toolUseId, tool, input }, at);
    const ended = (toolUseId: string) => noteBridgeEvent("w1", { kind: "tool", phase: "end", toolUseId });
    const dialog = (waiting: boolean, tool?: string, input?: unknown) =>
      noteBridgeEvent("w1", { kind: "permission", waiting, ...(tool ? { tool, input } : {}) });

    it("gives the question an AskUserQuestion call waits on, until the call ends", async () => {
      const { deps } = rig([chatShop()]);
      const shown = { ...COLOR, options: COLOR.options.map((option) => ({ ...option, preview: "<b>x</b>" })) };
      started("toolu_q", "AskUserQuestion", { questions: [shown] });
      dialog(true, "AskUserQuestion", { questions: [shown] });
      act("w1", "shop", "waiting");
      const asking = await chat(deps, { pane: "Ben" }, reader(page()).read);
      expect(asking.pending).toEqual({ kind: "question", toolUseId: "toolu_q", questions: [COLOR] });
      // The card shows it: no action line for it.
      expect(asking.items).toEqual(items);

      ended("toolu_q");
      dialog(false);
      act("w1", "shop", "working");
      expect((await chat(deps, { pane: "Ben" }, reader(page()).read)).pending).toBeNull();
    });

    it("gives the permission a dialog asks, titled as it would run, and the call once it runs", async () => {
      const { deps } = rig([chatShop()]);
      const input = { command: "pnpm test --run\npnpm build", description: "Run the tests" };
      started("toolu_b", "Bash", input);
      dialog(true, "Bash", input);
      act("w1", "shop", "waiting");
      const asking = await chat(deps, { pane: "Ben" }, reader(page()).read);
      expect(asking.pending).toEqual({
        kind: "permission",
        tool: "Bash",
        title: "Run pnpm test --run",
        detail: "pnpm test --run\npnpm build",
      });
      // Held by the dialog, it has not run yet.
      expect(asking.items).toEqual(items);

      dialog(false);
      act("w1", "shop", "working");
      const running = await chat(deps, { pane: "Ben" }, reader(page()).read);
      expect(running.pending).toBeNull();
      expect(running.items).toEqual([
        ...items,
        {
          id: "toolu_b",
          at: 5000,
          kind: "action",
          tool: "Bash",
          title: "Ran pnpm test --run",
          detail: "pnpm test --run\npnpm build",
          state: "running",
        },
      ]);
    });

    it("names an edit's file in the session's folder, with its diff", async () => {
      const { deps } = rig([chatShop()]);
      dialog(true, "Edit", { file_path: "c:\\wt\\ben\\src\\cart.ts", old_string: "a\nb", new_string: "c" });
      act("w1", "shop", "waiting");
      const asking = await chat(deps, { pane: "Ben" }, reader(page()).read);
      expect(asking.pending).toEqual({ kind: "permission", tool: "Edit", title: "Edit src/cart.ts", detail: "- a\n- b\n+ c" });
    });

    it("lists the calls running after the transcript's, on the newest page, each once", async () => {
      const { deps } = rig([chatShop()]);
      act("w1", "shop", "working");
      started("t1", "Bash", { command: "pnpm test" }, 4000);
      started("toolu_r", "Read", { file_path: "C:\\wt\\ben\\src\\x.ts" }, 6000);
      started("toolu_m", "mcp__hangar__board_list_tasks", {}, 7000);
      const newest = await chat(deps, { pane: "Ben" }, reader(page()).read);
      expect(newest.items).toEqual([
        ...items,
        { id: "toolu_r", at: 6000, kind: "action", tool: "Read", title: "Read src/x.ts", state: "running" },
        { id: "toolu_m", at: 7000, kind: "action", tool: "mcp__hangar__board_list_tasks", title: "Used board_list_tasks", state: "running" },
      ]);
      const older = await chat(deps, { pane: "Ben", before: "4096" }, reader(page()).read);
      expect(older.items).toEqual(items);

      ended("toolu_r");
      noteBridgeEvent("w1", { kind: "turn", phase: "end", reason: "aborted", durationMs: 1, answer: "" });
      expect((await chat(deps, { pane: "Ben" }, reader(page()).read)).items).toEqual(items);
    });

    it("leaves the transcript to speak when the mod reported nothing", async () => {
      const { deps } = rig([chatShop()]);
      act("w1", "shop", "waiting");
      noteBridgeEvent("orch", { kind: "permission", waiting: true, tool: "Bash", input: { command: "ls" } });
      const openCall = { tool: "Bash", title: "Ran pnpm test", detail: "pnpm test" };
      expect((await chat(deps, { pane: "Ben" }, reader(page({ openCall })).read)).pending).toEqual({
        kind: "permission",
        ...openCall,
      });
    });
  });
});

/** A project put to sleep: none of its panes has a terminal until it opens. */
const billing = () =>
  workspace("asleepWs", "billing-api", [
    { ...pane("noe", "Noé", "orchestrator"), sessionId: "sess-noe" },
    pane("logs", "logs", undefined, "shell"),
  ]);

function asleepRig() {
  const asleep = rig([shop(), billing()]);
  asleep.live.delete("noe");
  asleep.live.delete("logs");
  return asleep;
}

/** What a pane's Claude Code says once its session is up. */
const hello = (paneId: string) =>
  noteBridgeEvent(paneId, { kind: "hello", sessionId: "sess", model: "opus", version: "2.1.0" });

describe("remote_wake", () => {
  it("wakes a project asleep as a click on the PC does: opened, and counted as used", async () => {
    const { deps, opened, touched, asleep } = asleepRig();
    expect(await runRemote("remote_wake", { workspace: "asleepWs" }, deps)).toEqual({ waking: true });
    expect(opened).toEqual(["asleepWs"]);
    expect(touched).toContain("asleepWs");
    expect(asleep.has("asleepWs")).toBe(false);
  });

  it("finds the project by its name too", async () => {
    const { deps, opened } = asleepRig();
    expect(await runRemote("remote_wake", { workspace: "Billing-API" }, deps)).toEqual({ waking: true });
    expect(opened).toEqual(["asleepWs"]);
  });

  it("leaves a project that is awake alone", async () => {
    const { deps, opened, touched } = asleepRig();
    expect(await runRemote("remote_wake", { workspace: "shop" }, deps)).toEqual({ waking: false });
    expect(opened).toEqual([]);
    expect(touched).toEqual([]);
  });

  it("names an unknown project", async () => {
    const { deps, opened } = asleepRig();
    await expect(runRemote("remote_wake", { workspace: "ghost" }, deps)).rejects.toThrow('No project "ghost"');
    await expect(runRemote("remote_wake", {}, deps)).rejects.toThrow("Name the project");
    expect(opened).toEqual([]);
  });
});

describe("sending to a project asleep", () => {
  it("queues a message to a Claude Code pane for its session, never typing it into the shell", async () => {
    const { deps, typed, queued, opened, touched } = asleepRig();
    expect(await runRemote("remote_send", { pane: "Noé", text: "Ship the invoices" }, deps)).toEqual({
      sent: true,
      queued: true,
    });
    // As the person's own: no `from`, so the session reads it as typed by them.
    expect(queued).toEqual(["noe:Ship the invoices"]);
    expect(typed).toEqual([]);
    expect(opened).toEqual(["asleepWs"]);
    expect(touched).toContain("asleepWs");
  });

  it("keeps queueing while Claude Code resumes in its new terminal, and types once it is up", async () => {
    const { deps, typed, queued } = asleepRig();
    await runRemote("remote_wake", { workspace: "asleepWs" }, deps);
    // The terminal is there, the shell runs, Claude Code has not said hello yet.
    expect(await runRemote("remote_send", { pane: "Noé", text: "first" }, deps)).toEqual({ sent: true, queued: true });
    expect(await runRemote("remote_manager", { text: "status?" }, deps)).toEqual({ sent: true });
    expect(queued).toEqual(["noe:first"]);
    expect(typed).toEqual(["paste:pm:status?", 'write:pm:"\\r"']);

    hello("noe");
    expect(await runRemote("remote_send", { pane: "Noé", text: "second" }, deps)).toEqual({ sent: true });
    expect(queued).toEqual(["noe:first"]);
    expect(typed.slice(2)).toEqual(["paste:noe:second", 'write:noe:"\\r"']);
  });

  it("types into a pane the mod reports up, hello or not", async () => {
    const { deps, typed, queued } = asleepRig();
    await runRemote("remote_wake", { workspace: "asleepWs" }, deps);
    act("noe", "asleepWs", "idle");
    await runRemote("remote_send", { pane: "Noé", text: "hi" }, deps);
    expect(queued).toEqual([]);
    expect(typed).toEqual(["paste:noe:hi", 'write:noe:"\\r"']);
  });

  it("types into a shell pane once its terminal and its shell are there", async () => {
    const { deps, typed, opened, queued } = asleepRig();
    let naps = 0;
    const slow: ControlDeps = {
      ...deps,
      openWorkspace: (id) => void opened.push(id),
      // Its terminal mounts a moment after the workspace opens, its shell a moment later still.
      terminal: async (id) =>
        naps >= 1
          ? { paste: (text: string) => void typed.push(`paste:${id}:${text} after ${naps} naps`), read: () => [], applicationCursor: false }
          : null,
      alive: async () => naps >= 3,
      sleep: async () => void naps++,
    };
    expect(await runRemote("remote_send", { pane: "logs", text: "tail app.log" }, slow)).toEqual({ sent: true });
    expect(opened).toEqual(["asleepWs"]);
    expect(queued).toEqual([]);
    expect(typed).toEqual(["paste:logs:tail app.log after 3 naps", 'write:logs:"\\r"']);
  });

  it("refuses keys to a Claude Code pane not up yet, and wakes it", async () => {
    const { deps, typed, queued, opened } = asleepRig();
    await expect(runRemote("remote_send", { pane: "Noé", keys: ["escape"] }, deps)).rejects.toThrow("was asleep");
    expect(opened).toEqual(["asleepWs"]);
    await expect(runRemote("remote_send", { pane: "Noé", keys: ["enter"] }, deps)).rejects.toThrow(
      '"Noé" is resuming its session',
    );
    await expect(runRemote("remote_send", { pane: "Noé", text: "draft", submit: false }, deps)).rejects.toThrow(
      "Nothing was sent",
    );
    expect(typed).toEqual([]);
    expect(queued).toEqual([]);
  });

  it("without the mod to hold it, wakes the project and asks to send again", async () => {
    const { deps, typed, queued, opened } = asleepRig();
    deps.snapshot().settings.claudeBridge = false;
    await expect(runRemote("remote_send", { pane: "Noé", text: "hi" }, deps)).rejects.toThrow("was asleep");
    expect(typed).toEqual([]);
    expect(queued).toEqual([]);
    expect(opened).toEqual(["asleepWs"]);
  });

  it("delivers a question's answers through the queue, as the person would write them", async () => {
    const { deps, typed, queued, opened } = asleepRig();
    const question = {
      toolUseId: "toolu_q",
      questions: [
        { question: "Which currency?", header: "Currency", multiSelect: false, options: [{ label: "EUR", description: "" }] },
        {
          question: "Which colors?",
          header: "Colors",
          multiSelect: true,
          options: [
            { label: "Red", description: "" },
            { label: "Blue", description: "" },
          ],
        },
      ],
    };
    const read: ChatReader = async () => ({ items: [], older: false, cursor: null, question, openCall: null });
    const answers = [{ selected: ["EUR"] }, { selected: ["Blue", "Red"], other: "and green" }];
    expect(
      await runRemote("remote_answer", { pane: "Noé", toolUseId: "toolu_q", answers }, deps, { read: async () => [] }, read),
    ).toEqual({ answered: true, queued: true });
    expect(queued).toEqual(["noe:Which currency?\n→ EUR\n\nWhich colors?\n→ Red, Blue, and green"]);
    expect(typed).toEqual([]);
    expect(opened).toEqual(["asleepWs"]);
  });

  it("wakes the project for a permission, and says nothing waits for one any more", async () => {
    const { deps, typed, opened } = asleepRig();
    await expect(runRemote("remote_permission", { pane: "Noé", allow: true }, deps)).rejects.toThrow("was asleep");
    expect(typed).toEqual([]);
    expect(opened).toEqual(["asleepWs"]);
  });

  it("shows a pane asleep with an empty screen, not as gone", async () => {
    const { deps, opened } = asleepRig();
    expect(await runRemote("remote_screen", { pane: "Noé" }, deps)).toEqual({
      paneId: "noe",
      pane: "Noé",
      workspace: "billing-api",
      agent: "claude",
      activity: null,
      text: "",
    });
    // Reading is not using it.
    expect(opened).toEqual([]);
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

describe("remote_stats", () => {
  const HOUR = 3_600_000;
  // A Wednesday afternoon, on the PC's clock.
  const NOW = new Date(2026, 9, 28, 15, 0).getTime();
  /** 10 a.m., `days` days before today. */
  const daysAgo = (days: number) => addDays(startOfDay(NOW), -days) + 10 * HOUR;

  function bucket(session: number, at: number, model: number, patch: Partial<StatsBucket> = {}): StatsBucket {
    return {
      session,
      at,
      model,
      input: 0,
      output: 0,
      cacheWrite: 0,
      cacheRead: 0,
      cost: 0,
      costInput: 0,
      costOutput: 0,
      costCacheWrite: 0,
      costCacheRead: 0,
      calls: 0,
      turns: 0,
      activeMs: 0,
      ...patch,
    };
  }

  const FOLDERS = ["C:\\dev\\shop", "C:\\dev\\shop-worktrees\\feature", "C:\\elsewhere\\scratch"];

  function usage(buckets: StatsBucket[], cwds = FOLDERS): UsageStats {
    return {
      generatedAt: NOW - 5_000,
      scan: { files: 3, parsed: 1, reused: 2, failed: 0, bytes: 1_000, ms: 40, cachedOnly: false },
      sources: [],
      activeGapCapMs: 5 * 60_000,
      sessions: cwds.map((cwd, index) => ({
        agent: index === 2 ? "codex" : "claude",
        id: `s${index}`,
        cwd,
        title: `Session ${index}`,
        firstAt: NOW - 50 * 24 * HOUR,
        lastAt: NOW,
      })),
      models: [
        { id: "claude-opus-5-5", priced: true },
        { id: "gpt-5.6-sol", priced: true },
        { id: "local-llama", priced: false },
      ],
      buckets,
    };
  }

  /**
   * Shop's own session today, one in a worktree of it three days ago, Codex
   * in a folder of no workspace ten days ago, and a model without a price
   * forty days ago.
   */
  const rows = () => [
    bucket(0, daysAgo(0), 0, { input: 1_000, output: 500, cacheRead: 3_000, cost: 1.5, calls: 3 }),
    bucket(0, daysAgo(0), -1, { turns: 2, activeMs: 30 * 60_000 }),
    bucket(1, daysAgo(3), 0, { input: 2_000, output: 1_000, cost: 3, calls: 2 }),
    bucket(1, daysAgo(3), -1, { turns: 1, activeMs: 60 * 60_000 }),
    bucket(2, daysAgo(10), 1, { input: 10_000, cost: 0.5, calls: 1 }),
    bucket(2, daysAgo(10), -1, { turns: 1, activeMs: 20 * 60_000 }),
    bucket(0, daysAgo(40), 2, { input: 50_000, calls: 4 }),
  ];

  function sources(data: UsageStats, patch: Partial<StatsSources> = {}): StatsSources {
    return {
      usage: async () => data,
      worktrees: async (cwd) => (cwd === "C:\\dev\\shop" ? ["C:/dev/shop", "C:/dev/shop-worktrees/feature"] : []),
      plan: () => null,
      now: () => NOW,
      ...patch,
    };
  }

  const stats = async (args: Record<string, unknown>, from: StatsSources) =>
    (await runRemote("remote_stats", args, rig([shop()]).deps, undefined, undefined, from)) as RemoteStats;

  it("sums up the last 7 days the statistics page's way, a worktree's sessions in their workspace", async () => {
    const answer = await stats({ period: "7d" }, sources(usage(rows())));
    expect(answer).toMatchObject({
      period: "7d",
      measure: "tokens",
      readAt: NOW - 5_000,
      cachedOnly: false,
      granularity: "day",
      range: { from: addDays(startOfDay(NOW), -6), to: addDays(startOfDay(NOW), 1), days: 7 },
      totals: { tokens: 7_500, cacheRead: 3_000, cost: 4.5, activeMs: 90 * 60_000, sessions: 2, turns: 3 },
      previous: { tokens: 10_000, cost: 0.5, sessions: 1, turns: 1 },
      activeDays: 2,
    });
    expect(answer.series).toHaveLength(7);
    expect(answer.series[6]).toEqual({ at: startOfDay(NOW), value: 4_500 });
    expect(answer.series[3]).toEqual({ at: addDays(startOfDay(NOW), -3), value: 3_000 });
    expect(answer.series.filter((point) => point.value === 0)).toHaveLength(5);
    expect(answer.projects).toEqual({
      rows: [{ key: "ws:shop", label: "Shop", tokens: 7_500, cost: 4.5, activeMs: 90 * 60_000, sessions: 2, share: 1 }],
      other: null,
    });
    expect(answer.agents.rows.map((row) => [row.label, row.tokens, row.sessions])).toEqual([["Claude Code", 7_500, 2]]);
    expect(answer.models.rows.map((row) => [row.key, row.label, row.share])).toEqual([
      ["claude-opus-5-5", modelLabel("claude-opus-5-5"), 1],
    ]);
    // Never the hourly rows themselves.
    expect(JSON.stringify(answer)).not.toContain("costCacheRead");
  });

  it("ranks and draws by the measure asked for", async () => {
    const byTokens = await stats({ period: "30d", measure: "tokens" }, sources(usage(rows())));
    const byCost = await stats({ period: "30d", measure: "cost" }, sources(usage(rows())));
    expect(byTokens.projects.rows.map((row) => [row.label, row.share])).toEqual([
      ["scratch", 10_000 / 17_500],
      ["Shop", 7_500 / 17_500],
    ]);
    expect(byCost.projects.rows.map((row) => [row.label, row.share])).toEqual([
      ["Shop", 0.9],
      ["scratch", 0.1],
    ]);
    expect(byCost.agents.rows.map((row) => row.label)).toEqual(["Claude Code", "Codex"]);
    expect(byCost.series).toHaveLength(30);
    expect(byCost.series[29].value).toBe(1.5);
    expect(byTokens.series[29].value).toBe(4_500);
    expect(byCost.measure).toBe("cost");
  });

  it("takes 30 days and tokens by default, 90 days and all of it when asked", async () => {
    const plain = await stats({}, sources(usage(rows())));
    expect([plain.period, plain.measure, plain.range.days]).toEqual(["30d", "tokens", 30]);
    expect(plain.totals.tokens).toBe(17_500);

    const quarter = await stats({ period: "90d" }, sources(usage(rows())));
    expect(quarter.range.days).toBe(90);
    expect(quarter.totals.tokens).toBe(67_500);
    expect(quarter.previous).toMatchObject({ tokens: 0, sessions: 0 });

    const all = await stats({ period: "all" }, sources(usage(rows())));
    expect(all).toMatchObject({
      range: { from: addDays(startOfDay(NOW), -40), days: 41 },
      firstAt: daysAgo(40),
      previous: null,
      unpricedTokens: 50_000,
    });
    expect(all.series).toHaveLength(41);
    expect(all.models.rows.find((row) => row.key === "local-llama")).toMatchObject({ unpriced: true, cost: 0 });
  });

  it("folds the rows past the sixth into one", async () => {
    const folders = Array.from({ length: 8 }, (_, index) => `D:\\p${index}`);
    const data = usage(
      folders.map((_, index) => bucket(index, daysAgo(1), 0, { input: (index + 1) * 100 })),
      folders,
    );
    const answer = await stats({ period: "7d" }, sources(data));
    expect(answer.projects.rows.map((row) => row.label)).toEqual(["p7", "p6", "p5", "p4", "p3"]);
    expect(answer.projects.other).toMatchObject({ count: 3, tokens: 600, cost: 0, activeMs: 0 });
    expect(answer.projects.other?.share).toBeCloseTo(600 / 3_600);
  });

  it("answers zeros and empty rankings without any data", async () => {
    const answer = await stats({ period: "30d" }, sources(usage([])));
    expect(answer).toMatchObject({
      firstAt: null,
      totals: { tokens: 0, cost: 0, activeMs: 0, sessions: 0, turns: 0 },
      previous: { tokens: 0 },
      activeDays: 0,
      projects: { rows: [], other: null },
      agents: { rows: [], other: null },
      models: { rows: [], other: null },
      unpricedTokens: 0,
      plan: null,
    });
    expect(answer.series).toHaveLength(30);
    expect(answer.series.every((point) => point.value === 0)).toBe(true);
    const all = await stats({ period: "all" }, sources(usage([])));
    expect(all.range).toEqual({ from: startOfDay(NOW), to: addDays(startOfDay(NOW), 1), days: 1 });
  });

  it("says nothing of the plan until a session reported it, then its windows, whole, with when they reset", async () => {
    expect((await stats({}, sources(usage(rows())))).plan).toBeNull();
    const reported = sources(usage(rows()), {
      plan: () => ({
        at: NOW - 60_000,
        limits: [
          { kind: "five_hour", percentUsed: 34.4, resetsAt: "2026-10-28T17:00:00Z" },
          { kind: "seven_day", percentUsed: 140, resetsAt: null },
          { kind: "seven_day_opus", percentUsed: 12, resetsAt: "not a date" },
        ],
      }),
    });
    expect((await stats({}, reported)).plan).toEqual({
      at: NOW - 60_000,
      windows: [
        { kind: "five_hour", percentUsed: 34, resetsAt: Date.parse("2026-10-28T17:00:00Z") },
        { kind: "seven_day", percentUsed: 100, resetsAt: null },
        { kind: "seven_day_opus", percentUsed: 12, resetsAt: null },
      ],
    });
  });

  it("keeps the plan its sessions' mods report; one without limits does not erase it", async () => {
    const limits = [{ kind: "five_hour", percentUsed: 81, resetsAt: "2026-10-28T19:30:00Z" }];
    const reports = (rateLimits: typeof limits) =>
      ({ kind: "usage", context: { tokens: 1, window: 200_000, percent: 0 }, rateLimits, costUsd: null }) as const;
    noteBridgeEvent("orch", reports(limits), NOW - 1_000);
    noteBridgeEvent("w1", reports([]), NOW - 500);
    const answer = await stats({}, { ...sources(usage([])), plan: statsSources.plan });
    expect(answer.plan).toEqual({
      at: NOW - 1_000,
      windows: [{ kind: "five_hour", percentUsed: 81, resetsAt: Date.parse("2026-10-28T19:30:00Z") }],
    });
  });

  it("refuses a period or a measure it does not know", async () => {
    await expect(stats({ period: "1y" }, sources(usage([])))).rejects.toThrow(
      'Unknown period "1y". One of: 7d, 30d, 90d, all.',
    );
    await expect(stats({ measure: "time" }, sources(usage([])))).rejects.toThrow(
      'Unknown measure "time". One of: tokens, cost.',
    );
  });

  it("places sessions by the workspace's own folders when git cannot list its worktrees", async () => {
    const answer = await stats(
      { period: "7d" },
      sources(usage(rows()), { worktrees: () => Promise.reject(new Error("no git")) }),
    );
    expect(answer.projects.rows.map((row) => row.label)).toEqual(["Shop", "feature"]);
  });
});

describe("usageReader", () => {
  const data = (cachedOnly: boolean, generatedAt: number) =>
    ({ generatedAt, scan: { cachedOnly } }) as unknown as UsageStats;
  const never = () => new Promise<void>(() => {});

  it("reuses a fresh scan, and scans again once it is not", async () => {
    let clock = 0;
    const read = vi.fn(async () => data(false, clock));
    const reader = usageReader(read, { now: () => clock, freshMs: 60_000, wait: never });
    expect((await reader()).generatedAt).toBe(0);
    clock = 59_000;
    expect((await reader()).generatedAt).toBe(0);
    clock = 61_000;
    expect((await reader()).generatedAt).toBe(61_000);
    expect(read).toHaveBeenCalledTimes(2);
  });

  it("joins a scan under way rather than starting a second", async () => {
    let finish!: (value: UsageStats) => void;
    const read = vi.fn(() => new Promise<UsageStats>((resolve) => (finish = resolve)));
    const reader = usageReader(read, { wait: never });
    const first = reader();
    const second = reader();
    finish(data(false, 1));
    expect(await first).toBe(await second);
    expect(read).toHaveBeenCalledTimes(1);
  });

  it("answers from the cache when the scan takes too long, and from the scan once it is done", async () => {
    let finish!: (value: UsageStats) => void;
    const read = vi.fn((args: { cachedOnly?: boolean }) =>
      args.cachedOnly ? Promise.resolve(data(true, 1)) : new Promise<UsageStats>((resolve) => (finish = resolve)),
    );
    let late!: () => void;
    const reader = usageReader(read, { wait: () => new Promise<void>((resolve) => (late = resolve)) });
    const answer = reader();
    await vi.waitFor(() => expect(late).toBeTypeOf("function"));
    late();
    expect((await answer).scan.cachedOnly).toBe(true);
    finish(data(false, 2));
    expect((await reader()).scan.cachedOnly).toBe(false);
    expect(read.mock.calls.filter(([args]) => !args.cachedOnly)).toHaveLength(1);
  });

  it("passes a failed scan on, and scans again on the next call", async () => {
    const read = vi.fn().mockRejectedValueOnce(new Error("no transcripts")).mockResolvedValue(data(false, 3));
    const reader = usageReader(read, { wait: never });
    await expect(reader()).rejects.toThrow("no transcripts");
    expect((await reader()).generatedAt).toBe(3);
  });
});
