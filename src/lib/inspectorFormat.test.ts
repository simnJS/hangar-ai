import { describe, expect, it } from "vitest";
import {
  formatCost,
  formatElapsed,
  formatPercent,
  formatSince,
  planUsage,
  prettyModel,
  priorityBars,
  shortTaskId,
  spacedUnit,
  usageTone,
} from "./inspectorFormat";
import type { Translator } from "../i18n";

/** Echoes the key and its parameters, so a test reads which wording won. */
const t = ((key: string, params?: Record<string, string | number>) =>
  params ? `${key}(${Object.values(params).join(",")})` : key) as Translator;

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

describe("board task", () => {
  it("maps the 0–9 priority onto three bars", () => {
    expect(priorityBars(0)).toBe(1);
    expect(priorityBars(1)).toBe(2);
    expect(priorityBars(2)).toBe(3);
    expect(priorityBars(9)).toBe(3);
  });

  it("shortens a UUID but not an id that is already short", () => {
    expect(shortTaskId("e6f34373-d46e-483d-ad2f-d399520ee40c")).toBe("e6f343");
    expect(shortTaskId("14")).toBe("14");
  });
});

describe("numbers", () => {
  it("writes a cost the way the locale does", () => {
    expect(formatCost(1.84, "fr")).toMatch(/^1,84\s\$$/);
    expect(formatCost(1.84, "en")).toBe("$1.84");
    expect(formatCost(0.004, "en")).toBe("$0.004");
  });

  it("writes a percentage the way the locale does", () => {
    expect(formatPercent(34, "fr")).toMatch(/^34\s%$/);
    expect(formatPercent(34, "en")).toBe("34%");
  });

  it("spaces a token unit in French only", () => {
    expect(spacedUnit("84k", "fr")).toBe("84\u202fk");
    expect(spacedUnit("1.2M", "fr")).toBe("1.2\u202fM");
    expect(spacedUnit("84k", "en")).toBe("84k");
    expect(spacedUnit("512", "fr")).toBe("512");
  });

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

describe("time", () => {
  it("words a duration", () => {
    expect(formatElapsed(30_000, t)).toBe("inspector.elapsedNow");
    expect(formatElapsed(25 * 60_000, t)).toBe("inspector.elapsedMinutes(25)");
    expect(formatElapsed(72 * 60_000, t)).toBe("inspector.elapsedHours(1,12)");
    expect(formatElapsed(26 * 3_600_000, t)).toBe("inspector.elapsedDays(1,2)");
  });

  it("words a start time relative to now", () => {
    const now = new Date(2026, 9, 26, 15, 0).getTime();
    expect(formatSince(now - 20_000, now, t, "en")).toBe("inspector.justNow");
    expect(formatSince(now - 12 * 60_000, now, t, "en")).toBe("inspector.minutesAgo(12)");
    expect(formatSince(new Date(2026, 9, 26, 13, 18).getTime(), now, t, "fr")).toBe(
      "inspector.todayAt(13:18)",
    );
    expect(formatSince(new Date(2026, 9, 25, 18, 2).getTime(), now, t, "fr")).toBe(
      "inspector.yesterdayAt(18:02)",
    );
    expect(formatSince(new Date(2026, 9, 20, 9, 5).getTime(), now, t, "fr")).toMatch(
      /^inspector\.dateAt\(20 oct\.,09:05\)$/,
    );
  });
});
