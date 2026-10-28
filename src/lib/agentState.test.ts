import { describe, expect, it } from "vitest";
import { activityAtHello } from "./agentState";

describe("activityAtHello", () => {
  it("drops the work guessed from a starting session's output", () => {
    expect(activityAtHello("working")).toBe("idle");
  });

  it("leaves a wait, a hand-back and an idle pane as they are", () => {
    expect(activityAtHello("waiting")).toBe("waiting");
    expect(activityAtHello("yours")).toBe("yours");
    expect(activityAtHello("idle")).toBe("idle");
  });
});
