import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ApiError, createClient } from "./api";
import { createMock, WAKE_MS } from "./mock";
import { failWake, settleWakes, startWake, wakeOf, WAKE_TIMEOUT_MS, type Wakes } from "./wake";

const ASLEEP = { asleep: true };
const AWAKE = { asleep: false };

describe("where a project stands", () => {
  it("is asleep, with its button, until someone asks", () => {
    expect(wakeOf(ASLEEP, undefined, 1_000)).toEqual({ state: "asleep", failed: null });
  });

  it("is waking from the tap until the PC shows it awake", () => {
    const wakes = startWake({}, "ws", 1_000);
    expect(wakeOf(ASLEEP, wakes.ws, 1_000)).toEqual({ state: "waking" });
    expect(wakeOf(ASLEEP, wakes.ws, 1_000 + WAKE_TIMEOUT_MS - 1)).toEqual({ state: "waking" });
    expect(wakeOf(AWAKE, wakes.ws, 2_000)).toEqual({ state: "awake" });
  });

  it("gets its button back when the call fails, or the PC never wakes it", () => {
    const failed = failWake(startWake({}, "ws", 1_000), "ws", new ApiError("offline", "network"), 1_200);
    expect(wakeOf(ASLEEP, failed.ws, 1_300)).toEqual({ state: "asleep", failed: "call" });
    const late = startWake({}, "ws", 1_000);
    expect(wakeOf(ASLEEP, late.ws, 1_000 + WAKE_TIMEOUT_MS)).toEqual({ state: "asleep", failed: "timeout" });
  });

  it("is awake, without a button, whatever was tried", () => {
    expect(wakeOf(AWAKE, undefined, 1_000)).toEqual({ state: "awake" });
    expect(wakeOf(AWAKE, { at: 1, error: new Error("x") }, 1_000)).toEqual({ state: "awake" });
  });
});

describe("the tries at waking", () => {
  it("keep the one under way, so a second tap does not restart its clock", () => {
    const first = startWake({}, "ws", 1_000);
    expect(startWake(first, "ws", 5_000)).toBe(first);
  });

  it("start over after a failure, or once the last one timed out", () => {
    const failed = failWake({}, "ws", new Error("x"), 1_000);
    expect(startWake(failed, "ws", 2_000)).toEqual({ ws: { at: 2_000 } });
    const late = startWake({}, "ws", 1_000);
    expect(startWake(late, "ws", 1_000 + WAKE_TIMEOUT_MS)).toEqual({ ws: { at: 1_000 + WAKE_TIMEOUT_MS } });
  });

  it("end once the PC shows the project awake, or no longer lists it", () => {
    const wakes: Wakes = { a: { at: 1 }, b: { at: 1 }, gone: { at: 1 } };
    expect(
      settleWakes(wakes, [
        { id: "a", asleep: false },
        { id: "b", asleep: true },
      ]),
    ).toEqual({ b: { at: 1 } });
  });

  it("stay the same object while nothing settles, so nothing renders again", () => {
    const wakes: Wakes = { b: { at: 1 } };
    expect(settleWakes(wakes, [{ id: "b", asleep: true }])).toBe(wakes);
  });
});

describe("waking a project of the fake PC", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  /** Lets the fake PC's latency pass. */
  async function settle<T>(promise: Promise<T>, ms = 400): Promise<T> {
    promise.catch(() => {});
    await vi.advanceTimersByTimeAsync(ms);
    return promise;
  }

  const billing = async (client: ReturnType<typeof createClient>) =>
    (await settle(client.state())).projects.find((project) => project.id === "ws-billing")!;

  it("shows the button asleep, « waking » after the tap, then nothing once awake", async () => {
    const client = createClient({ fetch: createMock("default").fetch });
    let wakes: Wakes = {};

    const before = await billing(client);
    expect(before).toMatchObject({ asleep: true, open: false });
    expect(wakeOf(before, wakes[before.id], Date.now()).state).toBe("asleep");

    wakes = startWake(wakes, before.id, Date.now());
    expect(await settle(client.wake(before.id))).toEqual({ waking: true });
    const waking = await billing(client);
    wakes = settleWakes(wakes, [waking]);
    expect(waking.asleep).toBe(true);
    expect(wakeOf(waking, wakes[waking.id], Date.now()).state).toBe("waking");

    await vi.advanceTimersByTimeAsync(WAKE_MS);
    const after = await billing(client);
    wakes = settleWakes(wakes, [after]);
    expect(after).toMatchObject({ asleep: false, open: true });
    expect(after.panes.find((pane) => pane.name === "Noé")?.activity).toBe("idle");
    expect(wakeOf(after, wakes[after.id], Date.now()).state).toBe("awake");
    expect(wakes).toEqual({});
  });

  it("says an awake project needs no waking, and refuses one it does not know", async () => {
    const client = createClient({ fetch: createMock("default").fetch });
    expect(await settle(client.wake("ws-iabench"))).toEqual({ waking: false });
    await expect(settle(client.wake("ghost"))).rejects.toMatchObject({ kind: "refused" });
  });

  it("wakes a project for a message to one of its agents, which answers once back", async () => {
    const client = createClient({ fetch: createMock("default").fetch });
    expect(await settle(client.send({ pane: "Noé", text: "Where are the refunds?", submit: true }))).toEqual({
      sent: true,
      queued: true,
    });
    expect((await billing(client)).asleep).toBe(true);
    // Keys have nothing to wait for them.
    await expect(settle(client.send({ pane: "Noé", keys: ["escape"] }))).rejects.toMatchObject({ kind: "refused" });

    await vi.advanceTimersByTimeAsync(WAKE_MS + 3_000);
    expect((await billing(client)).asleep).toBe(false);
    const screen = await settle(client.screen("Noé"));
    expect(screen.text).toContain("> Where are the refunds?");
  });
});
