import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ApiError, createClient } from "./api";
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
    expect(state.waiting.map((item) => item.kind).sort()).toEqual(["needsYou", "pane", "readyToLand"]);
    expect(state.manager).toMatchObject({ name: "Max" });
    for (const item of state.waiting) expect(state.projects.some((project) => project.id === item.workspaceId)).toBe(true);
  });

  it("answers a message, then hands back", async () => {
    const client = fakePc();
    await settle(client.send({ pane: "Cora", text: "Ship it", submit: true }));
    expect(await settle(client.screen("Cora"))).toMatchObject({ activity: "working" });
    await vi.advanceTimersByTimeAsync(3_000);
    const screen = await settle(client.screen("p-cora"));
    expect(screen.activity).toBe("yours");
    expect(screen.text).toContain("> Ship it");
  });

  it("takes a permission only from a pane that waits for one", async () => {
    const client = fakePc();
    await expect(settle(client.permission("p-cora", true))).rejects.toMatchObject({ kind: "refused" });
    expect(await settle(client.permission("p-ben", true))).toEqual({ answered: true });
    expect((await settle(client.state())).waiting.some((item) => item.kind === "pane")).toBe(false);
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
