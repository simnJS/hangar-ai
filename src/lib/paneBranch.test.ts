import { beforeEach, describe, expect, it, vi } from "vitest";
import type { RepoInfo } from "./git";

const gitRepoInfo = vi.fn<(path: string) => Promise<RepoInfo>>();
vi.mock("./git", () => ({ gitRepoInfo: (path: string) => gitRepoInfo(path) }));

import {
  PANE_BRANCH_FOCUS_MS,
  PANE_BRANCH_TTL_MS,
  folderKey,
  loadPaneBranch,
  paneBranchOf,
  peekPaneBranch,
  resetPaneBranchCache,
} from "./paneBranch";
import { en, fr, paneBranchText } from "../strings/paneBranch";

const info = (patch: Partial<RepoInfo> = {}): RepoInfo => ({
  installed: true,
  isRepo: true,
  root: "C:/r",
  mainRoot: "C:/r",
  isLinkedWorktree: false,
  branch: "main",
  head: "abc1234",
  ...patch,
});

beforeEach(() => {
  vi.useRealTimers();
  gitRepoInfo.mockReset();
  resetPaneBranchCache();
});

describe("paneBranchOf", () => {
  it("names the branch", () => {
    expect(paneBranchOf(info({ isLinkedWorktree: true }))).toEqual({
      kind: "branch",
      name: "main",
      linkedWorktree: true,
    });
  });
  it("gives the short sha of a detached head", () => {
    expect(paneBranchOf(info({ branch: null }))).toEqual({ kind: "detached", head: "abc1234" });
  });
  it("says nothing outside a repo, without git, or on an unborn head", () => {
    expect(paneBranchOf(info({ isRepo: false, branch: null }))).toBeNull();
    expect(paneBranchOf(info({ installed: false, isRepo: false }))).toBeNull();
    expect(paneBranchOf(info({ branch: null, head: null }))).toBeNull();
    expect(paneBranchOf(null)).toBeNull();
  });
});

describe("folderKey", () => {
  it("merges the spellings of one folder", () => {
    expect(folderKey("C:\\Repo\\app\\")).toBe(folderKey("c:/repo/app"));
    expect(folderKey("/home/a/")).toBe("/home/a");
    expect(folderKey("C:\\")).toBe("c:/");
  });
});

describe("loadPaneBranch", () => {
  it("makes one call for several panes on a folder", async () => {
    gitRepoInfo.mockResolvedValue(info());
    const answers = await Promise.all([
      loadPaneBranch("C:\\r"),
      loadPaneBranch("c:/r/"),
      loadPaneBranch("C:/r"),
    ]);
    expect(gitRepoInfo).toHaveBeenCalledTimes(1);
    expect(answers.every((a) => a?.kind === "branch")).toBe(true);
    await loadPaneBranch("C:/r");
    expect(gitRepoInfo).toHaveBeenCalledTimes(1);
  });

  it("asks once per folder", async () => {
    gitRepoInfo
      .mockResolvedValueOnce(info({ branch: "a" }))
      .mockResolvedValueOnce(info({ branch: "b" }));
    const [a, b] = await Promise.all([loadPaneBranch("C:/one"), loadPaneBranch("C:/two")]);
    expect(gitRepoInfo).toHaveBeenCalledTimes(2);
    expect(a).toMatchObject({ name: "a" });
    expect(b).toMatchObject({ name: "b" });
  });

  it("asks again once the answer is stale, and on a focus after the short delay", async () => {
    vi.useFakeTimers();
    gitRepoInfo.mockResolvedValue(info());
    await loadPaneBranch("C:/r");
    vi.advanceTimersByTime(PANE_BRANCH_FOCUS_MS - 1);
    await loadPaneBranch("C:/r", PANE_BRANCH_FOCUS_MS);
    expect(gitRepoInfo).toHaveBeenCalledTimes(1);
    vi.advanceTimersByTime(2);
    gitRepoInfo.mockResolvedValue(info({ branch: "other" }));
    await loadPaneBranch("C:/r", PANE_BRANCH_FOCUS_MS);
    expect(gitRepoInfo).toHaveBeenCalledTimes(2);
    expect(peekPaneBranch("C:/r")).toMatchObject({ name: "other" });
    await loadPaneBranch("C:/r");
    expect(gitRepoInfo).toHaveBeenCalledTimes(2);
    vi.advanceTimersByTime(PANE_BRANCH_TTL_MS);
    await loadPaneBranch("C:/r");
    expect(gitRepoInfo).toHaveBeenCalledTimes(3);
  });

  it("reads a failing call as nothing to show", async () => {
    gitRepoInfo.mockRejectedValue(new Error("boom"));
    expect(await loadPaneBranch("C:/r")).toBeNull();
  });
});

describe("strings", () => {
  it("has the same keys in both languages and fills the parameters", () => {
    expect(Object.keys(fr).sort()).toEqual(Object.keys(en).sort());
    expect(paneBranchText("fr", "paneBranch.detached", { head: "abc" })).toBe(
      "HEAD détachée sur abc",
    );
    expect(paneBranchText("en", "paneBranch.branch", { branch: "x" })).toBe("Git branch x");
  });
});
