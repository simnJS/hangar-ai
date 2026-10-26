import { describe, expect, it } from "vitest";
import { compactPath, shortDay, signerName, wasUpdated } from "./memoryFormat";

describe("compactPath", () => {
  it("leaves a path that fits alone", () => {
    expect(compactPath("C:\\dev\\hangar-ai", 30)).toBe("C:\\dev\\hangar-ai");
  });

  it("keeps the end of a long Windows path", () => {
    expect(compactPath("C:\\Users\\simnJS\\Documents\\IaBench", 22)).toBe("…\\Documents\\IaBench");
  });

  it("keeps the end of a long POSIX path", () => {
    expect(compactPath("/home/someone/work/clients/storefront", 24)).toBe("…/clients/storefront");
  });

  it("never drops the last segment, however long", () => {
    expect(compactPath("/srv/a-folder-name-longer-than-the-limit", 12)).toBe(
      "…/a-folder-name-longer-than-the-limit",
    );
  });
});

describe("signerName", () => {
  it("says nothing for the user, who is addressed as you", () => {
    expect(signerName("user")).toBeNull();
    expect(signerName("  ")).toBeNull();
  });

  it("names a bare agent id after the agent", () => {
    expect(signerName("claude")).toBe("Claude Code");
    expect(signerName("codex")).toBe("Codex");
  });

  it("keeps any other signature as it was written", () => {
    expect(signerName("Ava")).toBe("Ava");
  });
});

describe("shortDay", () => {
  const now = new Date(2026, 9, 26, 15, 0).getTime();

  it("says today for anything since midnight", () => {
    expect(shortDay(new Date(2026, 9, 26, 0, 5).getTime(), now, "fr")).toBe("today");
  });

  it("drops the year within the current one", () => {
    const text = shortDay(new Date(2026, 9, 21).getTime(), now, "fr");
    expect(text).toContain("21");
    expect(text).not.toContain("2026");
  });

  it("keeps the year before it", () => {
    expect(shortDay(new Date(2025, 2, 3).getTime(), now, "en")).toContain("2025");
  });
});

describe("wasUpdated", () => {
  it("treats an upsert right after creation as the first writing", () => {
    expect(wasUpdated(1_000, 31_000)).toBe(false);
    expect(wasUpdated(1_000, 1_000 + 3_600_000)).toBe(true);
  });
});
