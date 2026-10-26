import { describe, expect, it } from "vitest";
import { elapsedLabel, priorityKey, priorityLevel, shortTaskId } from "./boardFormat";
import type { Translator } from "../i18n";

/** Echoes the key and its parameters, so a test reads what was asked for. */
const t: Translator = (key, params) => (params ? `${key} ${JSON.stringify(params)}` : key);

describe("priorityLevel", () => {
  it("reads the default priority as medium", () => {
    expect(priorityLevel(1)).toBe(2);
    expect(priorityKey(1)).toBe("board.priority.medium");
  });

  it("reads zero, and anything nonsensical, as low", () => {
    expect(priorityLevel(0)).toBe(1);
    expect(priorityLevel(-3)).toBe(1);
    expect(priorityLevel(Number.NaN)).toBe(1);
    expect(priorityKey(0)).toBe("board.priority.low");
  });

  it("reads everything above the default as high", () => {
    expect(priorityLevel(2)).toBe(3);
    expect(priorityLevel(9)).toBe(3);
    expect(priorityLevel(255)).toBe(3);
    expect(priorityKey(5)).toBe("board.priority.high");
  });
});

describe("shortTaskId", () => {
  it("keeps the first four characters of a UUID", () => {
    expect(shortTaskId("3f2a9c1e-0b7d-4c55-9a43-2f1e8d0c6b11")).toBe("3f2a");
  });

  it("skips the dashes so a short first group cannot leave one in", () => {
    expect(shortTaskId("ab-cdef")).toBe("abcd");
  });

  it("leaves a short id whole", () => {
    expect(shortTaskId("09")).toBe("09");
    expect(shortTaskId("-")).toBe("-");
  });
});

describe("elapsedLabel", () => {
  it("rounds anything under a minute to now", () => {
    expect(elapsedLabel(59_000, t)).toBe("board.elapsedNow");
    expect(elapsedLabel(-5_000, t)).toBe("board.elapsedNow");
  });

  it("counts minutes, then hours with padded minutes, then days", () => {
    expect(elapsedLabel(24 * 60_000, t)).toBe('board.elapsedMinutes {"m":24}');
    expect(elapsedLabel(72 * 60_000, t)).toBe('board.elapsedHours {"h":1,"m":"12"}');
    expect(elapsedLabel(65 * 60_000, t)).toBe('board.elapsedHours {"h":1,"m":"05"}');
    expect(elapsedLabel(27 * 3600_000, t)).toBe('board.elapsedDays {"d":1,"h":3}');
  });
});
