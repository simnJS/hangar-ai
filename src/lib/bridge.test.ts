import { describe, expect, it } from "vitest";
import { carryKey, stashQueue, takeStashedQueue } from "./bridge";

const brief = { id: "q1", text: "do the thing", from: "Jack" };

describe("queue carried over a restart", () => {
  it("hands the stashed prompts to the new pane, once", () => {
    const key = carryKey("ws", "Tara");
    stashQueue(key, [brief], 1000);
    expect(takeStashedQueue(key, 2000)).toEqual([brief]);
    expect(takeStashedQueue(key, 2000)).toEqual([]);
  });

  it("lets go of a stash that wore off", () => {
    const key = carryKey("ws", "Max");
    stashQueue(key, [brief], 1000);
    expect(takeStashedQueue(key, 1000 + 60_000)).toEqual([]);
  });

  it("keeps panes of different names or workspaces apart", () => {
    stashQueue(carryKey("ws", "Ava"), [brief], 1000);
    expect(takeStashedQueue(carryKey("ws", "Bob"), 1500)).toEqual([]);
    expect(takeStashedQueue(carryKey("other", "Ava"), 1500)).toEqual([]);
    expect(takeStashedQueue(carryKey("ws", "Ava"), 1500)).toEqual([brief]);
  });

  it("an empty queue stashes nothing", () => {
    const key = carryKey("ws", "Eve");
    stashQueue(key, [brief], 1000);
    stashQueue(key, [], 1100);
    expect(takeStashedQueue(key, 1200)).toEqual([]);
  });
});
