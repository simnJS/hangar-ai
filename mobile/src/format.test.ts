import { describe, expect, it } from "vitest";
import { ago, clock, dayHeading, daysBefore, duration, fileSize, moment } from "./format";

const NBSP = " ";
const MIN = 60_000;
const HOUR = 60 * MIN;
// Local times, so the tests read the same in every time zone.
const now = new Date(2026, 9, 28, 20, 15).getTime();

describe("durations", () => {
  it("counts in the largest whole unit", () => {
    expect(duration(0, "fr")).toBe(`1${NBSP}s`);
    expect(duration(42_000, "en")).toBe(`42${NBSP}s`);
    expect(duration(4 * MIN + 59_000, "fr")).toBe(`4${NBSP}min`);
    expect(duration(3 * HOUR + 10 * MIN, "en")).toBe(`3${NBSP}h`);
    expect(duration(50 * HOUR, "fr")).toBe(`2${NBSP}j`);
    expect(duration(50 * HOUR, "en")).toBe(`2${NBSP}d`);
  });
});

describe("moments", () => {
  it("counts calendar days, not 24 h spans", () => {
    expect(daysBefore(new Date(2026, 9, 27, 23, 50).getTime(), now)).toBe(1);
    expect(daysBefore(new Date(2026, 9, 28, 0, 5).getTime(), now)).toBe(0);
  });

  it("says a time today, yesterday, a weekday, then a date", () => {
    const today = new Date(2026, 9, 28, 14, 32).getTime();
    expect(moment(today, now, "fr")).toBe(clock(today, "fr"));
    expect(clock(today, "fr")).toBe("14:32");
    const yesterday = new Date(2026, 9, 27, 18, 4).getTime();
    expect(moment(yesterday, now, "fr")).toBe("hier 18:04");
    expect(moment(yesterday, now, "en")).toBe(`yesterday ${clock(yesterday, "en")}`);
    expect(moment(new Date(2026, 9, 26, 9, 0).getTime(), now, "fr")).toBe("lundi 9:00");
    expect(moment(new Date(2026, 9, 12, 9, 0).getTime(), now, "fr")).toBe("12 oct. 9:00");
  });

  it("says how long ago while it is recent", () => {
    expect(ago(now - 20_000, now, "fr")).toBe("à l'instant");
    expect(ago(now - 12 * MIN, now, "fr")).toBe(`il y a 12${NBSP}min`);
    expect(ago(now - 12 * MIN, now, "en")).toBe(`12${NBSP}min ago`);
    expect(ago(now - 3 * HOUR, now, "fr")).toBe(`il y a 3${NBSP}h`);
    expect(ago(new Date(2026, 9, 27, 22, 32).getTime(), now, "fr")).toBe("hier 22:32");
  });

  it("heads each day of a list", () => {
    expect(dayHeading(now - HOUR, now, "fr")).toBe("Aujourd'hui");
    expect(dayHeading(now - 24 * HOUR, now, "en")).toBe("Yesterday");
    expect(dayHeading(new Date(2026, 9, 24, 12).getTime(), now, "fr")).toBe("Samedi 24 octobre");
  });
});

describe("file sizes", () => {
  it("reads in the phone's language", () => {
    expect(fileSize(840, "en")).toBe(`840${NBSP}B`);
    expect(fileSize(18_400, "fr")).toBe(`18${NBSP}ko`);
    expect(fileSize(2_500_000, "fr")).toBe(`2,4${NBSP}Mo`);
    expect(fileSize(2_500_000, "en")).toBe(`2.4${NBSP}MB`);
    expect(fileSize(25 * 1024 * 1024, "en")).toBe(`25${NBSP}MB`);
  });
});
