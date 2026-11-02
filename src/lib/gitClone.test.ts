import { describe, expect, it, vi } from "vitest";

const invoke = vi.fn();
let listener: ((event: { payload: unknown }) => void) | null = null;
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invoke(...args),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: async (_name: string, handler: (event: { payload: unknown }) => void) => {
    listener = handler;
    return () => {
      listener = null;
    };
  },
}));

import {
  CLONE_PROGRESS_EVENT,
  cleanRepoInput,
  defaultCloneFolder,
  gitClone,
  gitCloneCancel,
  gitCloneDiscard,
  gitRepoOwnership,
  gitRepoParse,
  newCloneJobId,
  onCloneProgress,
  overallPercent,
  progressLabel,
} from "./gitClone";

describe("cleanRepoInput", () => {
  it("trims the ends and leaves the inside alone", () => {
    expect(cleanRepoInput("  octo/hello\n")).toBe("octo/hello");
    expect(cleanRepoInput("octo/hel lo")).toBe("octo/hel lo");
  });
});

describe("defaultCloneFolder", () => {
  it("is the repository's name", () => {
    expect(defaultCloneFolder({ name: "hello" })).toBe("hello");
  });
});

describe("newCloneJobId", () => {
  it("is unique and prefixed", () => {
    const a = newCloneJobId();
    expect(a).toMatch(/^clone-.+/);
    expect(newCloneJobId()).not.toBe(a);
  });
});

describe("progressLabel", () => {
  it("shows the percentage when there is one", () => {
    expect(progressLabel({ phase: "Receiving objects", percent: 45 })).toBe(
      "Receiving objects 45 %",
    );
    expect(progressLabel({ phase: "Receiving objects", percent: null })).toBe(
      "Receiving objects",
    );
  });
});

describe("overallPercent", () => {
  it("maps the phases onto one bar", () => {
    expect(overallPercent({ phase: "Receiving objects", percent: 0 })).toBe(0);
    expect(overallPercent({ phase: "Receiving objects", percent: 50 })).toBe(40);
    expect(overallPercent({ phase: "Receiving objects", percent: 100 })).toBe(80);
    expect(overallPercent({ phase: "Resolving deltas", percent: 50 })).toBe(90);
    expect(overallPercent({ phase: "Resolving deltas", percent: 100 })).toBe(100);
    expect(overallPercent({ phase: "Updating files", percent: 10 })).toBe(100);
  });
  it("is null for an unknown phase or no percentage, and clamps", () => {
    expect(overallPercent({ phase: "Counting objects", percent: 100 })).toBeNull();
    expect(overallPercent({ phase: "Receiving objects", percent: null })).toBeNull();
    expect(overallPercent({ phase: "Receiving objects", percent: 250 })).toBe(80);
  });
});

describe("commands", () => {
  it("passes the cleaned input and camelCase arguments", async () => {
    invoke.mockResolvedValue({ status: "refused", reason: "x" });
    await gitRepoParse("  octo/hello \n");
    expect(invoke).toHaveBeenLastCalledWith("git_repo_parse", { input: "octo/hello" });
    await gitRepoOwnership("octo", "github.com");
    expect(invoke).toHaveBeenLastCalledWith("git_repo_ownership", {
      owner: "octo",
      host: "github.com",
    });
    await gitClone("https://github.com/octo/hello.git", "C:/p", "hello", "job-1");
    expect(invoke).toHaveBeenLastCalledWith("git_clone", {
      url: "https://github.com/octo/hello.git",
      parent: "C:/p",
      name: "hello",
      jobId: "job-1",
    });
    await gitCloneCancel("job-1");
    expect(invoke).toHaveBeenLastCalledWith("git_clone_cancel", { jobId: "job-1" });
    await gitCloneDiscard("C:/p/hello");
    expect(invoke).toHaveBeenLastCalledWith("git_clone_discard", { path: "C:/p/hello" });
  });

  it("only hands over the progress of its own job", async () => {
    const seen: unknown[] = [];
    const stop = await onCloneProgress("job-1", (p) => seen.push(p));
    listener?.({ payload: { jobId: "job-2", phase: "x", percent: 1 } });
    listener?.({ payload: { jobId: "job-1", phase: "Receiving objects", percent: 3 } });
    expect(seen).toEqual([{ jobId: "job-1", phase: "Receiving objects", percent: 3 }]);
    stop();
    expect(listener).toBeNull();
    expect(CLONE_PROGRESS_EVENT).toBe("git-clone:progress");
  });
});
