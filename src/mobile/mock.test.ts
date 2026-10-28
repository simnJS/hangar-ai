import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ApiError, createClient } from "./api";
import { actionKind } from "./chat";
import { PUSH_KEY, createMock, scenarioOf } from "./mock";

/** The fake PC, reached through the real client, so it keeps to the contract. */
function fakePc(scenario: Parameters<typeof createMock>[0] = "default") {
  const mock = createMock(scenario);
  return createClient({ fetch: mock.fetch, fileUrl: mock.fileUrl });
}

/** Lets the fake PC's latency pass; a refusal is left for the test to read. */
async function settle<T>(promise: Promise<T>, ms = 400): Promise<T> {
  promise.catch(() => {});
  await vi.advanceTimersByTimeAsync(ms);
  return promise;
}

describe("the fake PC", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("knows its scenarios, and falls back to the usual one", () => {
    expect(scenarioOf("offline")).toBe("offline");
    expect(scenarioOf("")).toBe("default");
    expect(scenarioOf(null)).toBe("default");
  });

  it("describes projects that wait for the person, without the HQ", async () => {
    const client = fakePc();
    const state = await settle(client.state());
    expect(state.projects.map((project) => project.name)).toEqual(["IaBench", "storefront", "billing-api", "recipes-app"]);
    expect(state.waiting.map((item) => item.kind).sort()).toEqual(["needsYou", "pane", "pane", "readyToLand"]);
    expect(state.manager).toMatchObject({ name: "Max" });
    for (const item of state.waiting) expect(state.projects.some((project) => project.id === item.workspaceId)).toBe(true);
  });

  it("answers a message, then hands back", async () => {
    const client = fakePc();
    await settle(client.send({ pane: "Mia", text: "Ship it", submit: true }));
    expect(await settle(client.screen("Mia"))).toMatchObject({ activity: "working" });
    await vi.advanceTimersByTimeAsync(3_000);
    const screen = await settle(client.screen("p-mia"));
    expect(screen.activity).toBe("yours");
    expect(screen.text).toContain("> Ship it");
  });

  it("takes a permission only from a pane that waits for one", async () => {
    const client = fakePc();
    await expect(settle(client.permission("p-mia", true))).rejects.toMatchObject({ kind: "refused" });
    // Cora waits too, for answers to her questions.
    await expect(settle(client.permission("p-cora", true))).rejects.toMatchObject({ kind: "refused" });
    expect(await settle(client.permission("p-ben", true))).toEqual({ answered: true });
    expect((await settle(client.state())).waiting.some((item) => item.kind === "pane" && item.pane === "Ben")).toBe(false);
  });

  it("writes its history in the phone's language, task titles as they were written", async () => {
    for (const [locale, words] of [
      ["en", "Needs you: "],
      ["fr", "Vous attend : "],
    ] as const) {
      const mock = createMock("default", Date.now(), locale);
      const { entries } = await settle(createClient({ fetch: mock.fetch }).history(0));
      const ask = entries.find((entry) => entry.kind === "needsYou");
      expect(ask?.text).toBe(`${words}Should Tailscale Funnel stay out of scope, or be offered as an option?`);
    }
  });

  it("has nothing waiting in its empty scenario", async () => {
    expect((await settle(fakePc("empty").state())).waiting).toEqual([]);
  });

  it("asks to pair in its unpaired scenario, and refuses a wrong code", async () => {
    const client = fakePc("unpaired");
    expect((await settle(client.health())).paired).toBe(false);
    await expect(settle(client.state())).rejects.toMatchObject({ kind: "unpaired" });
    await expect(settle(client.pair("000000", "test"))).rejects.toBeInstanceOf(ApiError);
    await settle(client.pair("482913", "test"));
    expect((await settle(client.health())).paired).toBe(true);
  });

  it("does not answer at all in its offline scenario", async () => {
    const mock = createMock("offline");
    expect(mock.seed?.state.projects.length).toBeGreaterThan(0);
    await expect(createClient({ fetch: mock.fetch }).state()).rejects.toMatchObject({ kind: "offline" });
  });

  it("refuses the manager's messages when there is none", async () => {
    const client = fakePc("nomanager");
    expect((await settle(client.state())).manager).toBeNull();
    await expect(settle(client.manager("hello"))).rejects.toMatchObject({ kind: "refused" });
  });

  it("puts an uploaded file where the agent can read it", async () => {
    const client = fakePc();
    const { path } = await settle(client.upload("ws-iabench", new Blob(["x"]), "photo.jpg"), 1_000);
    expect(path).toMatch(/\.hangar\\uploads\\.*-photo\.jpg$/);
  });

  it("takes a push subscription like the PC, an https one only", async () => {
    const client = fakePc();
    expect(await settle(client.pushKey())).toEqual({ key: PUSH_KEY });
    const keys = { p256dh: PUSH_KEY, auth: "BTBZMqHH6r4Tts7J_aSIgg" };
    expect(await settle(client.pushSubscribe({ endpoint: "https://push.example/a", keys }, "en"))).toEqual({
      subscribed: true,
    });
    await expect(settle(client.pushSubscribe({ endpoint: "http://push.example/a", keys }, "en"))).rejects.toMatchObject({
      kind: "refused",
      code: "bad-subscription",
    });
    expect(await settle(client.pushUnsubscribe())).toEqual({ subscribed: false });
  });
});

describe("the fake PC's conversations", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  /** Every page of a pane's conversation, the latest first, as the phone asks for them. */
  async function allPages(client: ReturnType<typeof fakePc>, pane: string) {
    const pages = [await settle(client.chat(pane, { limit: 60 }))];
    while (pages[pages.length - 1].older) {
      const cursor = pages[pages.length - 1].cursor!;
      pages.push(await settle(client.chat(pane, { before: cursor, limit: 60 })));
    }
    return pages;
  }

  it("gives a long conversation in pages, oldest first, each page before the cursor", async () => {
    const pages = await allPages(fakePc(), "p-ada");
    expect(pages.length).toBeGreaterThanOrEqual(3);
    expect(pages[0].items).toHaveLength(60);
    expect(pages[pages.length - 1]).toMatchObject({ older: false, cursor: null });
    const items = pages.reverse().flatMap((page) => page.items);
    expect(new Set(items.map((item) => item.id)).size).toBe(items.length);
    for (let index = 1; index < items.length; index++) expect(items[index].at).toBeGreaterThanOrEqual(items[index - 1].at);
  });

  it("holds every kind of item, actions of every kind, one running and one failed", async () => {
    const items = (await allPages(fakePc(), "p-ada")).flatMap((page) => page.items);
    const kinds = new Set(items.map((item) => (item.kind === "user" ? `user:${item.from}` : item.kind)));
    expect([...kinds].sort()).toEqual(["action", "assistant", "notice", "user:hangar", "user:person"]);
    const actions = items.filter((item) => item.kind === "action");
    expect(new Set(actions.map((item) => actionKind(item.tool)))).toEqual(new Set(["edit", "command", "read", "search", "other"]));
    expect(actions.filter((item) => item.state === "running")).toHaveLength(1);
    expect(actions.some((item) => item.state === "error" && item.result?.ok === false)).toBe(true);
    // Markdown with a heading, a table and a link, for the renderer to show.
    expect(items.some((item) => item.kind === "assistant" && /^## /m.test(item.text) && item.text.includes("|---|"))).toBe(true);
  });

  it("has a pane that waits for a permission, and one that asks several questions", async () => {
    const client = fakePc();
    expect((await settle(client.chat("Ben"))).pending).toMatchObject({ kind: "permission", tool: "Bash" });
    const cora = await settle(client.chat("Cora"));
    expect(cora.pending?.kind).toBe("question");
    if (cora.pending?.kind !== "question") return;
    expect(cora.pending.questions.length).toBeGreaterThan(1);
    expect(cora.pending.questions.some((question) => question.multiSelect)).toBe(true);
    // Questions answered earlier carry what was chosen.
    const answered = cora.items.find((item) => item.kind === "question" && item.answer);
    expect(answered).toMatchObject({ answer: [{ selected: [], other: expect.any(String) }, { selected: ["For you", "Projects"] }] });
  });

  it("says why a pane has no chat: another agent, or no session yet", async () => {
    const client = fakePc();
    expect(await settle(client.chat("p-shell"))).toMatchObject({ items: [], reason: "notClaude", sessionId: null });
    expect(await settle(client.chat("Leo"))).toMatchObject({ items: [], reason: "notClaude" });
    expect(await settle(client.chat("Noé"))).toMatchObject({ items: [], reason: "noSession" });
    await expect(settle(client.chat("p-ada", { before: "nowhere" }))).rejects.toMatchObject({ kind: "refused", code: "bad-cursor" });
  });

  it("takes the answers to its question, and refuses answers that do not fit", async () => {
    const client = fakePc();
    const right = [{ selected: ["Submit now"] }, { selected: ["Screenshots", "Known gaps"] }, { selected: [], other: "Settings" }];
    await expect(settle(client.answer("Cora", "toolu_other", right))).rejects.toMatchObject({ kind: "refused", status: 409 });
    await expect(settle(client.answer("Cora", "toolu_cora_ship", right.slice(0, 2)))).rejects.toMatchObject({ status: 400 });
    await expect(
      settle(client.answer("Cora", "toolu_cora_ship", [{ selected: ["Submit now", "Wait for the screenshots"] }, ...right.slice(1)])),
    ).rejects.toMatchObject({ status: 400 });
    await expect(settle(client.answer("Cora", "toolu_cora_ship", [{ selected: ["Nope"] }, ...right.slice(1)]))).rejects.toMatchObject({
      status: 400,
    });
    expect(await settle(client.answer("Cora", "toolu_cora_ship", right))).toEqual({ answered: true });
    const after = await settle(client.chat("Cora"));
    expect(after.pending).toBeNull();
    expect(after.activity).toBe("working");
    expect(after.items.find((item) => item.kind === "question" && item.toolUseId === "toolu_cora_ship")).toMatchObject({ answer: right });
    await vi.advanceTimersByTimeAsync(3_000);
    expect((await settle(client.chat("Cora"))).items.slice(-1)[0]).toMatchObject({ kind: "assistant" });
  });

  it("writes what the person sends, its answer after a tool call, and an interruption", async () => {
    const client = fakePc();
    await settle(client.send({ pane: "Mia", text: "Ship it", submit: true }));
    expect((await settle(client.chat("Mia"))).items.slice(-1)[0]).toMatchObject({ kind: "user", from: "person", text: "Ship it" });
    await vi.advanceTimersByTimeAsync(1_000);
    expect((await settle(client.chat("Mia"))).items.slice(-1)[0]).toMatchObject({ kind: "action", state: "running" });
    await vi.advanceTimersByTimeAsync(2_000);
    expect((await settle(client.chat("Mia"))).items.slice(-2)).toMatchObject([{ kind: "action", state: "done" }, { kind: "assistant" }]);

    await settle(client.send({ pane: "Ada", keys: ["escape"] }));
    const ada = await settle(client.chat("Ada"));
    expect(ada.activity).toBe("yours");
    expect(ada.items.slice(-2)).toMatchObject([{ kind: "action", state: "error" }, { kind: "notice" }]);
  });

  it("runs the call that waited for permission once it is allowed", async () => {
    const client = fakePc();
    expect(await settle(client.permission("Ben", true))).toEqual({ answered: true });
    const ben = await settle(client.chat("Ben"));
    expect(ben.pending).toBeNull();
    expect(ben.items.slice(-1)[0]).toMatchObject({ kind: "action", tool: "Bash", state: "done", result: { ok: true } });
  });
});
