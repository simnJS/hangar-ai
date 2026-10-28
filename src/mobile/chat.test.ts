import { describe, expect, it } from "vitest";
import type { Chat, ChatItem, Pending } from "./api";
import {
  ACTIONS_SHOWN,
  ANSWER_SETTLES_MS,
  FOLLOW_SLACK_PX,
  OLDER_MARGIN_PX,
  OUTGOING_MAX_MS,
  TIME_GAP_MS,
  actionKind,
  actionLine,
  actionTitle,
  afterNewItems,
  afterScroll,
  anchoredScrollTop,
  atBottom,
  chatRows,
  chipOf,
  diffLine,
  diffLog,
  hangarText,
  isClaude,
  mergeLatest,
  mergeOlder,
  noteFolds,
  noticeText,
  pathParts,
  pendingKey,
  shouldLoadOlder,
  showsPending,
  stillSending,
  visibleActions,
  waitsElsewhere,
  type ActionItem,
  type ChatLog,
} from "./chat";
import { translator } from "./i18n";

const T0 = new Date(2026, 9, 28, 14, 0).getTime();

const user = (id: string, text: string, at = T0, from: "person" | "hangar" = "person"): ChatItem => ({
  id,
  at,
  kind: "user",
  text,
  from,
  images: 0,
});
const said = (id: string, text: string, at = T0): ChatItem => ({ id, at, kind: "assistant", text });
const action = (id: string, tool = "Bash", state: ActionItem["state"] = "done", at = T0): ActionItem => ({
  id,
  at,
  kind: "action",
  tool,
  title: `Ran ${id}`,
  state,
});

function page(items: ChatItem[], fields: Partial<Chat> = {}): Chat {
  return {
    paneId: "p-ada",
    pane: "Ada",
    workspace: "IaBench",
    agent: "claude",
    activity: "working",
    sessionId: "s1",
    items,
    older: false,
    cursor: null,
    pending: null,
    ...fields,
  };
}

const log = (items: ChatItem[], fields: Partial<ChatLog> = {}): ChatLog => ({
  sessionId: "s1",
  items,
  older: false,
  cursor: null,
  ...fields,
});

describe("what an item shows as", () => {
  it("gives each tool its kind of icon, any other tool the generic one", () => {
    expect(["Edit", "Write", "Bash", "Read", "Grep", "Glob", "WebSearch", "mcp__hangar__board_list_tasks", "Task"].map(actionKind)).toEqual([
      "edit",
      "edit",
      "command",
      "read",
      "search",
      "search",
      "search",
      "other",
      "other",
    ]);
  });

  it("puts the PC's English verbs in the phone's words, and leaves other lines as written", () => {
    const fr = translator("fr").t;
    expect(actionTitle("Edited src/a.ts", fr)).toBe("Modifié src/a.ts");
    expect(actionTitle("Ran pnpm test", fr)).toBe("Exécuté pnpm test");
    expect(actionTitle("Read 4 files", fr)).toBe("4 fichiers lus");
    expect(actionTitle("Read src/x.ts", fr)).toBe("Lu src/x.ts");
    // French quotes get their narrow no-break spaces, as every template does.
    expect(actionTitle("Searched for foo", fr)).toBe("Cherché « foo »");
    expect(actionTitle("Used project_list", fr)).toBe("Utilisé project_list");
    expect(actionTitle("Run the window ops' tests", fr)).toBe("Run the window ops' tests");
    // What fills the line is never typeset: a path's colon stays as it is.
    expect(actionTitle("Ran cargo test remote::", fr)).toBe("Exécuté cargo test remote::");
    expect(actionLine("Read 4 files", fr)).toEqual({ before: "", subject: "4", after: " fichiers lus" });
    expect(actionLine("Something else", fr)).toEqual({ before: "", subject: "Something else", after: "" });
    // The longer shapes the PC writes win over the shorter ones they start like.
    expect(actionTitle("Ran agent: review the diff", fr)).toBe("Agent lancé : review the diff");
    expect(actionTitle("Searched the web for tailscale serve", fr)).toBe("Cherché sur le web « tailscale serve »");
    expect(actionTitle("Used skill frontend-design", fr)).toBe("Utilisé le skill frontend-design");
    expect(actionTitle("Updated the to-do list", fr)).toBe("Liste de tâches mise à jour");
    expect(actionLine("Loaded tools", fr)).toEqual({ before: "Outils chargés", subject: "", after: "" });
    const en = translator("en").t;
    const titles = [
      "Edited src/a.ts",
      "Wrote src/b.ts",
      "Ran pnpm test",
      "Ran agent: review the diff",
      "Read 4 files",
      "Read src/x.ts",
      "Fetched https://example.com",
      "Searched the web for foo",
      "Searched for foo",
      "Used skill frontend-design",
      "Used project_list",
      "Updated the to-do list",
      "Loaded tools",
      "Presented a plan",
    ];
    for (const title of titles) expect(actionTitle(title, en)).toBe(title);
  });

  it("puts the PC's notices in the phone's words, an error Claude Code reported as written", () => {
    const fr = translator("fr").t;
    expect(noticeText("Conversation compacted", fr)).toBe("Conversation compactée");
    expect(noticeText("Turn interrupted", fr)).toBe("Réponse interrompue");
    expect(noticeText("Session resumed", fr)).toBe("Session reprise");
    expect(noticeText("API Error: 529 Overloaded", fr)).toBe("API Error: 529 Overloaded");
    expect(noticeText("constructor", fr)).toBe("constructor");
    expect(noticeText("Session resumed", translator("en").t)).toBe("Session resumed");
  });

  it("puts a path's file first, its folder after it", () => {
    expect(pathParts("src-tauri/src/remote/session.rs")).toEqual({ dir: "src-tauri/src/remote", base: "session.rs" });
    expect(pathParts("C:\\Users\\me\\a.txt")).toEqual({ dir: "C:\\Users\\me", base: "a.txt" });
    expect(pathParts("CHANGELOG.md")).toBeNull();
    expect(pathParts("cargo test remote::")).toBeNull();
    expect(pathParts("https://example.com/a")).toBeNull();
    expect(pathParts("src/")).toBeNull();
  });

  it("shows Hangar's messages as notes, without their [Hangar]", () => {
    expect(hangarText("[Hangar] Briefing — not typed by the person.")).toBe("Briefing — not typed by the person.");
    const rows = chatRows([user("u1", "[Hangar] From Jack", T0, "hangar"), user("u2", "Ship it")], null);
    expect(rows.map((row) => row.type)).toEqual(["time", "hangar", "user"]);
    expect(rows[1]).toMatchObject({ type: "hangar", text: "From Jack" });
  });

  it("folds a long note, not a short one", () => {
    expect(noteFolds("From Jack: on it.")).toBe(false);
    expect(noteFolds("a\nb\nc\nd\ne")).toBe(true);
    expect(noteFolds("x".repeat(400))).toBe(true);
  });

  it("puts actions in a row into one block, between the messages", () => {
    const rows = chatRows([said("a1", "Reading"), action("x1"), action("x2"), said("a2", "Done"), action("x3")], null);
    expect(rows.map((row) => row.type)).toEqual(["time", "assistant", "actions", "assistant", "actions"]);
    expect(rows[2]).toMatchObject({ items: [{ id: "x1" }, { id: "x2" }] });
  });

  it("puts the time above the first message, after a pause, and on a new day", () => {
    const rows = chatRows(
      [
        said("a1", "one", T0),
        said("a2", "two", T0 + TIME_GAP_MS - 1),
        said("a3", "three", T0 + 2 * TIME_GAP_MS),
        said("a4", "four", new Date(2026, 9, 29, 0, 5).getTime()),
      ],
      null,
    );
    expect(rows.filter((row) => row.type === "time").map((row) => row.key)).toEqual(["time-a1", "time-a3", "time-a4"]);
  });

  it("leaves out empty answers", () => {
    expect(chatRows([said("a1", "  \n"), said("a2", "Hello")], null).map((row) => row.type)).toEqual(["time", "assistant"]);
  });

  it("leaves out the question the card at the foot asks, not the ones answered", () => {
    const question = (toolUseId: string): ChatItem => ({ id: toolUseId, at: T0, kind: "question", toolUseId, questions: [] });
    const pending: Pending = { kind: "question", toolUseId: "q2", questions: [] };
    expect(chatRows([question("q1"), question("q2")], pending).map((row) => row.key)).toEqual(["time-q1", "question-q1"]);
  });

  it("leaves out the tool call that waits for permission, shown by its card", () => {
    const pending: Pending = { kind: "permission", tool: "Bash", title: "Run the tests", detail: "pnpm test" };
    const items = [action("x1"), action("x2", "Bash", "running")];
    expect(chatRows(items, pending).flatMap((row) => (row.type === "actions" ? row.items.map((item) => item.id) : []))).toEqual(["x1"]);
    // Another tool running is not the one that asks.
    const other = chatRows([action("x3", "Read", "running")], pending);
    expect(other[1]).toMatchObject({ items: [{ id: "x3" }] });
  });

  it("folds a long run of actions to its last few", () => {
    const run = ["a", "b", "c", "d", "e", "f"].map((id) => action(id));
    expect(visibleActions(run.slice(0, 4), false)).toEqual({ hidden: 0, shown: run.slice(0, 4) });
    const folded = visibleActions(run, false);
    expect(folded.hidden).toBe(run.length - ACTIONS_SHOWN);
    expect(folded.shown.map((item) => item.id)).toEqual(["d", "e", "f"]);
    expect(visibleActions(run, true)).toEqual({ hidden: 0, shown: run });
  });

  it("colours a diff's lines", () => {
    expect(["@@ -1 +1 @@", "+added", "-removed", " same", "+++ b/a.ts", "--- a/a.ts"].map(diffLine)).toEqual([
      "hunk",
      "add",
      "del",
      null,
      null,
      null,
    ]);
  });

  it("shows a chat for Claude Code only", () => {
    expect(isClaude("claude")).toBe(true);
    expect(["codex", "gemini", "shell", null, undefined].map(isClaude)).toEqual([false, false, false, false, false]);
  });
});

describe("the pages the phone holds", () => {
  it("starts with the first page", () => {
    const first = page([said("a1", "hi")], { older: true, cursor: "a1" });
    expect(mergeLatest(null, first)).toEqual({ log: log([said("a1", "hi")], { older: true, cursor: "a1" }), added: 0, reset: true });
  });

  it("adds what is new, and changes what changed, the cursor kept", () => {
    const held = log([said("a1", "hi"), action("x1", "Bash", "running")], { older: true, cursor: "a1" });
    const latest = page([action("x1", "Bash", "done"), said("a2", "Done")], { older: true, cursor: "x1" });
    const { log: merged, added } = mergeLatest(held, latest);
    expect(merged.items.map((item) => item.id)).toEqual(["a1", "x1", "a2"]);
    expect(merged.items[1]).toMatchObject({ state: "done" });
    expect(merged.items[0]).toBe(held.items[0]);
    expect(added).toBe(1);
    expect(merged.cursor).toBe("a1");
  });

  it("gives back the same log when nothing changed", () => {
    const held = log([said("a1", "hi"), said("a2", "there")]);
    const result = mergeLatest(held, page([said("a2", "there")]));
    expect(result.log).toBe(held);
    expect(result.added).toBe(0);
  });

  it("takes the PC's word from its first known item on: an item folded away goes", () => {
    const held = log([said("a1", "hi"), action("r1", "Read"), action("r2", "Read")]);
    const folded: ChatItem = { ...action("r1", "Read"), title: "Read 2 files" };
    const { log: merged } = mergeLatest(held, page([folded]));
    expect(merged.items.map((item) => item.id)).toEqual(["a1", "r1"]);
    expect(merged.items[1]).toMatchObject({ title: "Read 2 files" });
  });

  it("starts over on a new session, or when it missed too much", () => {
    const held = log([said("a1", "hi")]);
    expect(mergeLatest(held, page([said("b1", "new")], { sessionId: "s2" }))).toMatchObject({
      log: { sessionId: "s2", items: [{ id: "b1" }] },
      reset: true,
    });
    expect(mergeLatest(held, page([said("a9", "far")], { older: true, cursor: "a9" }))).toMatchObject({
      log: { items: [{ id: "a9" }], cursor: "a9", older: true },
      reset: true,
    });
  });

  it("fills an empty conversation with its first messages", () => {
    const { log: merged, added, reset } = mergeLatest(log([]), page([user("u1", "go")]));
    expect(merged.items.map((item) => item.id)).toEqual(["u1"]);
    expect([added, reset]).toEqual([1, false]);
  });

  it("puts an older page above, with its cursor", () => {
    const held = log([said("a3", "three")], { older: true, cursor: "a3" });
    const older = page([said("a1", "one"), said("a2", "two")], { older: true, cursor: "a1" });
    const { log: merged, added } = mergeOlder(held, older, "a3");
    expect(merged.items.map((item) => item.id)).toEqual(["a1", "a2", "a3"]);
    expect(merged).toMatchObject({ older: true, cursor: "a1" });
    expect(added).toBe(2);
    const last = mergeOlder(merged, page([said("a0", "zero")]), "a1").log;
    expect(last).toMatchObject({ older: false, cursor: null });
  });

  it("drops an older page that no longer fits", () => {
    const held = log([said("a3", "three")], { older: true, cursor: "a3" });
    expect(mergeOlder(held, page([said("a1", "one")], { sessionId: "s2" }), "a3").log).toBe(held);
    expect(mergeOlder(held, page([said("a1", "one")]), "a2").log).toBe(held);
  });
});

describe("following the newest message", () => {
  const metrics = (scrollTop: number, scrollHeight = 2000, clientHeight = 600) => ({ scrollTop, scrollHeight, clientHeight });

  it("counts as at the bottom within a few pixels of it", () => {
    expect(atBottom(metrics(1400))).toBe(true);
    expect(atBottom(metrics(1400 - FOLLOW_SLACK_PX))).toBe(true);
    expect(atBottom(metrics(1400 - FOLLOW_SLACK_PX - 1))).toBe(false);
  });

  it("follows what arrives at the bottom, counts the messages above it", () => {
    expect(afterNewItems({ following: true, unseen: 0 }, [said("a1", "hi")])).toEqual({ following: true, unseen: 0, scroll: true });
    expect(afterNewItems({ following: true, unseen: 0 }, [])).toMatchObject({ scroll: false });
    // Reading above: messages counted, actions not.
    expect(afterNewItems({ following: false, unseen: 1 }, [said("a2", "more"), action("x1")])).toEqual({
      following: false,
      unseen: 2,
      scroll: false,
    });
  });

  it("forgets what was unseen once the person is back at the bottom", () => {
    expect(afterScroll({ following: false, unseen: 3 }, metrics(1400))).toEqual({ following: true, unseen: 0 });
    expect(afterScroll({ following: true, unseen: 0 }, metrics(200))).toEqual({ following: false, unseen: 0 });
    expect(afterScroll({ following: false, unseen: 3 }, metrics(200))).toEqual({ following: false, unseen: 3 });
  });

  it("names its chip by what waits below", () => {
    const pending: Pending = { kind: "question", toolUseId: "q", questions: [] };
    expect(chipOf({ following: true, unseen: 4 }, pending)).toBeNull();
    expect(chipOf({ following: false, unseen: 4 }, pending)).toEqual({ key: "chat.toQuestion" });
    expect(chipOf({ following: false, unseen: 4 }, { kind: "permission", tool: "Bash", title: "", detail: "" })).toEqual({
      key: "chat.toPermission",
    });
    expect(chipOf({ following: false, unseen: 4 }, null)).toEqual({ key: "chat.newMessages", n: 4 });
    expect(chipOf({ following: false, unseen: 0 }, null)).toEqual({ key: "chat.toEnd" });
  });

  it("asks for the page before near the top, once, while there is one", () => {
    const more = log([said("a2", "x")], { older: true, cursor: "a2" });
    expect(shouldLoadOlder(metrics(OLDER_MARGIN_PX), more, false)).toBe(true);
    expect(shouldLoadOlder(metrics(OLDER_MARGIN_PX + 1), more, false)).toBe(false);
    expect(shouldLoadOlder(metrics(0), more, true)).toBe(false);
    expect(shouldLoadOlder(metrics(0), log([said("a1", "x")]), false)).toBe(false);
    expect(shouldLoadOlder(metrics(0), null, false)).toBe(false);
  });

  it("keeps what was read in place when a page goes above", () => {
    expect(anchoredScrollTop({ scrollTop: 40, scrollHeight: 2000 }, 3500)).toBe(1540);
  });

  it("tells a page put above from messages that arrived, and from a new list", () => {
    const before = [said("a2", "two"), said("a3", "three")];
    expect(diffLog(before, [said("a1", "one"), ...before])).toEqual({ prepended: 1, appended: [], reset: false });
    expect(diffLog(before, [...before, said("a4", "four")])).toMatchObject({ prepended: 0, appended: [{ id: "a4" }], reset: false });
    expect(diffLog([], before)).toMatchObject({ reset: true });
    expect(diffLog(before, [said("b1", "other")])).toMatchObject({ reset: true });
    expect(diffLog([], [])).toMatchObject({ reset: false });
  });
});

describe("what the person sent", () => {
  const sent = (text: string, after: string | null, at = T0) => ({ id: `out-${text}`, text, at, after });

  it("stays at the foot until the transcript holds it after where it was sent", () => {
    const outbox = [sent("Ship it", "a1")];
    expect(stillSending(outbox, [said("a1", "Ready?")], T0)).toBe(outbox);
    // The same words said before it was sent are not its copy.
    expect(stillSending(outbox, [user("u0", "Ship it"), said("a1", "Ready?")], T0)).toBe(outbox);
    expect(stillSending(outbox, [said("a1", "Ready?"), user("u1", " Ship it ")], T0)).toEqual([]);
  });

  it("matches two messages alike with two copies", () => {
    const outbox = [sent("yes", null), { ...sent("yes", null), id: "out-yes-2" }];
    expect(stillSending(outbox, [user("u1", "yes")], T0)).toEqual([outbox[1]]);
  });

  it("is let go after a while", () => {
    expect(stillSending([sent("hello", null)], [], T0 + OUTGOING_MAX_MS + 1)).toEqual([]);
  });
});

describe("what the pane waits for", () => {
  const question: Pending = { kind: "question", toolUseId: "q1", questions: [] };
  const permission: Pending = { kind: "permission", tool: "Bash", title: "Run the tests", detail: "pnpm test" };

  it("keys a question by its call, a permission by what it asks", () => {
    expect(pendingKey(question)).toBe("question:q1");
    expect(pendingKey(permission)).toBe("permission:Bash:Run the tests");
    expect(pendingKey(null)).toBeNull();
  });

  it("steps aside once answered, while the PC still reports it, a few seconds at most", () => {
    const answered = { key: "question:q1", at: T0 };
    expect(showsPending(question, null, T0)).toBe(true);
    expect(showsPending(question, answered, T0 + 1000)).toBe(false);
    expect(showsPending(question, answered, T0 + ANSWER_SETTLES_MS + 1)).toBe(true);
    // Something else asked meanwhile shows at once.
    expect(showsPending(permission, answered, T0 + 1000)).toBe(true);
    expect(showsPending(null, null, T0)).toBe(false);
  });

  it("points to the terminal when the pane waits for something the PC cannot say", () => {
    expect(waitsElsewhere("waiting", null, null, T0)).toBe(true);
    expect(waitsElsewhere("waiting", permission, null, T0)).toBe(false);
    expect(waitsElsewhere("working", null, null, T0)).toBe(false);
    // Not while an answer settles.
    expect(waitsElsewhere("waiting", null, { key: "question:q1", at: T0 }, T0 + 1000)).toBe(false);
  });
});
