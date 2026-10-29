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
    expect(scenarioOf("blocked")).toBe("blocked");
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

describe("the fake PC's statistics", () => {
  beforeEach(() => vi.useFakeTimers({ now: new Date(2026, 9, 29, 15, 0) }));
  afterEach(() => vi.useRealTimers());

  it("counts five months the PC's way, for every period and measure", async () => {
    const client = fakePc();
    for (const period of ["7d", "30d", "90d", "all"] as const) {
      for (const measure of ["tokens", "cost"] as const) {
        const stats = await settle(client.stats(period, measure), 600);
        expect(stats).toMatchObject({ period, measure, cachedOnly: false });
        // A column a day, or a week past four months: the first one partly before the period.
        const weeks = Math.ceil(stats.range.days / 7);
        if (stats.granularity === "day") expect(stats.series).toHaveLength(stats.range.days);
        else expect([weeks, weeks + 1]).toContain(stats.series.length);
        expect(stats.totals.tokens).toBeGreaterThan(0);
        expect(stats.totals.cost).toBeGreaterThan(0);
        // Heaviest first, by what was asked.
        const values = stats.projects.rows.map((row) => row[measure]);
        expect(values).toEqual([...values].sort((a, b) => b - a));
        expect(stats.projects.rows.length + (stats.projects.other ? 1 : 0)).toBeLessThanOrEqual(6);
      }
    }
    const month = await settle(client.stats("30d", "tokens"), 600);
    expect(month.projects.rows[0].label).toBe("IaBench");
    expect(month.agents.rows.map((row) => row.label)).toEqual(["Claude Code", "Codex"]);
    expect(month.plan?.windows.map((window) => [window.kind, window.percentUsed])).toEqual([
      ["five_hour", 72],
      ["seven_day", 38],
    ]);
    const all = await settle(client.stats("all", "cost"), 600);
    expect(all.granularity).toBe("week");
    expect(all.previous).toBeNull();
    expect(all.projects.other?.count).toBeGreaterThan(0);
    expect(all.unpricedTokens).toBeGreaterThan(0);
  });

  it("knows nothing on a PC where no agent has worked yet", async () => {
    const stats = await settle(fakePc("nostats").stats("30d", "tokens"), 600);
    expect(stats).toMatchObject({ firstAt: null, plan: null, totals: { tokens: 0 }, projects: { rows: [], other: null } });
  });

  it("refuses a period it does not know, as the PC does", async () => {
    const client = fakePc();
    await expect(settle(client.stats("1y" as never, "tokens"), 600)).rejects.toMatchObject({ kind: "refused" });
  });
});

describe("the fake PC's delivery ready to merge", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("says what it brings: a few files, a check that failed once then passed, a review with notes", async () => {
    const client = fakePc();
    const state = await settle(client.state());
    const card = state.waiting.find((item) => item.kind === "readyToLand")!;
    const delivery = await settle(client.delivery(card.taskId!));
    expect(delivery).toMatchObject({ worker: "Dana", target: "main", waiting: true, merge: "4f9c2ab", check: { passed: true } });
    expect(delivery.review?.notes).toContain("Not blocking");
    expect(delivery.earlier.map((attempt) => attempt.outcome)).toEqual(["checkFailed"]);
    expect(delivery.files.length).toBeGreaterThanOrEqual(5);
    expect(delivery.files.some((file) => file.binary)).toBe(true);
    // Its worker's pane is still there, with its worktree.
    expect(state.projects[0].panes.some((pane) => pane.name === "Dana" && pane.role === "worker")).toBe(true);

    const qr = await settle(client.deliveryFile(card.taskId!, "src/lib/qr.ts"));
    expect(qr).toMatchObject({ binary: false, truncated: false });
    const added = qr.patch.split("\n").filter((line) => line.startsWith("+")).length;
    expect(delivery.files.find((file) => file.path === "src/lib/qr.ts")?.additions).toBe(added);
    const big = await settle(client.deliveryFile(card.taskId!, "src/lib/qr-tables.ts"));
    expect(big.truncated).toBe(true);
    expect(big.patch.length).toBeLessThanOrEqual(200 * 1024);
    await expect(settle(client.deliveryFile(card.taskId!, "nowhere.ts"))).rejects.toMatchObject({ kind: "refused" });
  });

  it("lands it after a few seconds of checking, and the task is done", async () => {
    const client = fakePc();
    expect(await settle(client.land("t-settings", "4f9c2ab"))).toEqual({ landed: false, stage: "checking" });
    expect(await settle(client.delivery("t-settings"))).toMatchObject({ waiting: false, stage: "checking" });
    await vi.advanceTimersByTimeAsync(2_400);
    expect(await settle(client.land("t-settings", "4f9c2ab"))).toEqual({ landed: false, stage: "landing" });
    await vi.advanceTimersByTimeAsync(2_000);
    expect(await settle(client.land("t-settings", "4f9c2ab"))).toEqual({ landed: true, target: "main" });

    const state = await settle(client.state());
    expect(state.waiting.some((item) => item.kind === "readyToLand")).toBe(false);
    expect(state.projects[0].panes.some((pane) => pane.name === "Dana")).toBe(false);
    await expect(settle(client.delivery("t-settings"))).rejects.toMatchObject({ kind: "refused", detail: expect.stringContaining("waits to land") });
  });

  it("refuses a delivery other than the one shown, and says why one does not land", async () => {
    const client = fakePc("blocked");
    await expect(settle(client.land("t-settings", "0000000"))).rejects.toMatchObject({
      kind: "refused",
      detail: expect.stringContaining("delivered again since you opened it"),
    });
    await settle(client.land("t-settings", "4f9c2ab"));
    await vi.advanceTimersByTimeAsync(4_500);
    await expect(settle(client.land("t-settings", "4f9c2ab"))).rejects.toMatchObject({
      kind: "refused",
      detail: expect.stringContaining("Local changes in main's checkout are in the way"),
    });
    // It still waits, and says what stood in the way.
    expect(await settle(client.delivery("t-settings"))).toMatchObject({ waiting: true, inTheWay: ["src/i18n.ts"] });
  });

  it("sends it back to Dana with what to fix, and it still waits until she delivers again", async () => {
    const client = fakePc();
    await expect(settle(client.rework("t-settings", "  ", "4f9c2ab"))).rejects.toMatchObject({ kind: "refused" });
    expect(await settle(client.rework("t-settings", "The QR code is too small on an iPhone SE.", "4f9c2ab"))).toEqual({ sent: true });
    const screen = await settle(client.screen("Dana"));
    expect(screen.activity).toBe("working");
    expect(screen.text).toContain("> The QR code is too small on an iPhone SE.");
    expect(await settle(client.delivery("t-settings"))).toMatchObject({ waiting: true });
  });

  it("has nothing to merge when nothing waits", async () => {
    await expect(settle(fakePc("empty").delivery("t-settings"))).rejects.toMatchObject({ kind: "refused" });
  });
});
