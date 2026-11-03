import { describe, expect, it } from "vitest";
import { planUsage, prettyModel, usageTone } from "./inspectorFormat";

describe("usageTone", () => {
  it("turns amber at 70 % and red at 90 %", () => {
    expect(usageTone(69)).toBe("normal");
    expect(usageTone(70)).toBe("warn");
    expect(usageTone(89)).toBe("warn");
    expect(usageTone(90)).toBe("danger");
  });
});

describe("prettyModel", () => {
  it("names Claude models the way people say them", () => {
    expect(prettyModel("claude-sonnet-4-5-20250929")).toBe("Sonnet 4.5");
    expect(prettyModel("claude-opus-4-1-20250805")).toBe("Opus 4.1");
    expect(prettyModel("claude-opus-4-20250514")).toBe("Opus 4");
    expect(prettyModel("claude-opus-5")).toBe("Opus 5");
    expect(prettyModel("claude-sonnet-4-5[1m]")).toBe("Sonnet 4.5");
    expect(prettyModel("claude-3-7-sonnet-20250219")).toBe("Sonnet 3.7");
    expect(prettyModel("claude-3-haiku-20240307")).toBe("Haiku 3");
    expect(prettyModel("opus")).toBe("Opus");
  });

  it("leaves anything else as it came", () => {
    expect(prettyModel("gpt-5-codex")).toBe("gpt-5-codex");
    expect(prettyModel(null)).toBeNull();
    expect(prettyModel("")).toBeNull();
  });
});

describe("planUsage", () => {
  it("reads a plan window, clamped", () => {
    const limits = [
      { kind: "five_hour", percentUsed: 34.4, resetsAt: null },
      { kind: "seven_day", percentUsed: 140, resetsAt: null },
    ];
    expect(planUsage(limits, "five_hour")?.percentUsed).toBe(34);
    expect(planUsage(limits, "seven_day")?.percentUsed).toBe(100);
    expect(planUsage(limits, "opus")).toBeNull();
    expect(planUsage(null, "five_hour")).toBeNull();
  });
});
