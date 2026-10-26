import { describe, expect, it } from "vitest";
import {
  fileBaseName,
  firstLine,
  gaugeLevel,
  newlyQueued,
  paneGlyphState,
  shellShortName,
  shortSessionId,
} from "./PaneStatus";

describe("PaneStatus", () => {
  it("shows the shell's life before the agent's state", () => {
    expect(paneGlyphState("starting", "working")).toBe("starting");
    expect(paneGlyphState("exited", "waiting")).toBe("exited");
    expect(paneGlyphState("running", "waiting")).toBe("waiting");
    // A plain shell reports nothing and rests as idle.
    expect(paneGlyphState("running", null)).toBe("idle");
  });

  it("warms the gauge from 70 % and heats it from 90 %", () => {
    expect(gaugeLevel(0)).toBe("ok");
    expect(gaugeLevel(69)).toBe("ok");
    expect(gaugeLevel(70)).toBe("warn");
    expect(gaugeLevel(89)).toBe("warn");
    expect(gaugeLevel(90)).toBe("hot");
    expect(gaugeLevel(100)).toBe("hot");
  });

  it("names a shell after its program", () => {
    const shell = (program: string) => ({ id: "x", label: "Label", program, args: [] });
    expect(shellShortName(shell("pwsh.exe"))).toBe("pwsh");
    expect(shellShortName(shell("C:\\Program Files\\Git\\bin\\bash.exe"))).toBe("bash");
    expect(shellShortName(shell("/bin/zsh"))).toBe("zsh");
    expect(shellShortName(shell("CMD.EXE"))).toBe("cmd");
    expect(shellShortName(shell(""))).toBe("Label");
    expect(shellShortName(null)).toBeNull();
  });

  it("shortens session ids", () => {
    expect(shortSessionId("01J8Z4-pty-bridge")).toBe("01J8Z4");
    expect(shortSessionId("abc")).toBe("abc");
  });

  it("keeps the first line that says something", () => {
    expect(firstLine("\n  \n  Done: tests pass\nmore")).toBe("Done: tests pass");
    expect(firstLine("")).toBe("");
    expect(firstLine("x".repeat(10), 5)).toBe("xxxx…");
  });

  it("takes a file's name off either kind of path", () => {
    expect(fileBaseName("src/lib/layout.ts")).toBe("layout.ts");
    expect(fileBaseName("C:\\dev\\app\\main.rs")).toBe("main.rs");
    expect(fileBaseName("README.md")).toBe("README.md");
    expect(fileBaseName("src/board/")).toBe("board");
  });

  it("finds the prompts that joined the queue", () => {
    const a = { id: "1", text: "a" };
    const b = { id: "2", text: "b" };
    const c = { id: "3", text: "c" };
    expect(newlyQueued([], [a, b])).toEqual([a, b]);
    expect(newlyQueued([a, b], [b, c])).toEqual([c]);
    expect(newlyQueued([a, b], [b])).toEqual([]);
  });
});
