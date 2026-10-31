import { describe, expect, it, vi } from "vitest";
import {
  launchAccount,
  launchedAccount,
  noteLaunchedAccount,
  readAccountChoice,
  resolveAccount,
  subscribeLaunchedAccounts,
  type ClaudeAccount,
} from "./accounts";

const acc = (id: string, isMain = false): ClaudeAccount => ({ id, label: id, isMain, loggedIn: true });
const accounts = [acc("main", true), acc("pro"), acc("perso")];

describe("readAccountChoice", () => {
  it("keeps a non-empty string", () => {
    expect(readAccountChoice("pro")).toBe("pro");
    expect(readAccountChoice("  pro ")).toBe("pro");
  });
  it("turns anything malformed into null", () => {
    for (const bad of [undefined, null, "", "   ", 3, true, {}, [], ["pro"]]) {
      expect(readAccountChoice(bad)).toBeNull();
    }
  });
});

describe("resolveAccount", () => {
  it("falls to main with nothing chosen", () => {
    expect(resolveAccount(accounts, {}, {}, {})).toEqual({ id: "main", source: "main" });
    expect(resolveAccount(accounts)).toEqual({ id: "main", source: "main" });
  });
  it("prefers pane over workspace over default", () => {
    const s = { claudeAccount: "perso" };
    const w = { claudeAccount: "pro" };
    expect(resolveAccount(accounts, s)).toEqual({ id: "perso", source: "default" });
    expect(resolveAccount(accounts, s, w)).toEqual({ id: "pro", source: "workspace" });
    expect(resolveAccount(accounts, s, w, { claudeAccount: "main" })).toEqual({
      id: "main",
      source: "pane",
    });
  });
  it("ignores malformed choices", () => {
    expect(resolveAccount(accounts, { claudeAccount: "pro" }, { claudeAccount: 4 as never })).toEqual({
      id: "pro",
      source: "default",
    });
  });
  it("falls back from an unknown id and says which", () => {
    expect(resolveAccount(accounts, {}, { claudeAccount: "gone" })).toEqual({
      id: "main",
      source: "main",
      missing: "gone",
    });
    expect(resolveAccount(accounts, { claudeAccount: "perso" }, { claudeAccount: "gone" })).toEqual({
      id: "perso",
      source: "default",
      missing: "gone",
    });
  });
});

describe("resolveAccount and a hand-edited state.json", () => {
  it("knows main even when the list does not", () => {
    expect(resolveAccount([], { claudeAccount: "main" })).toEqual({ id: "main", source: "default" });
    expect(resolveAccount([], {}, { claudeAccount: " main " })).toEqual({ id: "main", source: "workspace" });
  });
  it("takes malformed values at every level as no choice", () => {
    const junk = [42, true, {}, ["pro"], "", "   ", null, undefined];
    for (const bad of junk) {
      const level = { claudeAccount: bad } as never;
      expect(resolveAccount(accounts, level, level, level)).toEqual({ id: "main", source: "main" });
      expect(resolveAccount(accounts, { claudeAccount: "perso" }, level, level)).toEqual({
        id: "perso",
        source: "default",
      });
    }
  });
  it("survives levels that are not objects at all", () => {
    expect(resolveAccount(accounts, null, undefined, null)).toEqual({ id: "main", source: "main" });
    expect(resolveAccount(accounts, "pro" as never, 7 as never, [] as never)).toEqual({
      id: "main",
      source: "main",
    });
  });
  it("trims a choice before looking it up", () => {
    expect(resolveAccount(accounts, {}, {}, { claudeAccount: " pro " })).toEqual({ id: "pro", source: "pane" });
  });
  it("reports only the first gone account and keeps falling", () => {
    expect(
      resolveAccount(accounts, { claudeAccount: "old-default" }, { claudeAccount: "old-ws" }, { claudeAccount: "old-pane" }),
    ).toEqual({ id: "main", source: "main", missing: "old-pane" });
  });
});

describe("launchAccount", () => {
  it("does not ask for the accounts when only main is chosen", async () => {
    const list = vi.fn(async () => accounts);
    expect(await launchAccount({ claudeAccount: "main" }, {}, {}, list)).toEqual({ id: "main", source: "default" });
    expect(await launchAccount(undefined, undefined, undefined, list)).toEqual({ id: "main", source: "main" });
    expect(await launchAccount({ claudeAccount: 3 } as never, null, { claudeAccount: "" }, list)).toEqual({
      id: "main",
      source: "main",
    });
    expect(list).not.toHaveBeenCalled();
  });
  it("resolves against the accounts when one is named", async () => {
    const list = vi.fn(async () => accounts);
    expect(await launchAccount({ claudeAccount: "main" }, { claudeAccount: "pro" }, {}, list)).toEqual({
      id: "pro",
      source: "workspace",
    });
    expect(await launchAccount({}, { claudeAccount: "pro" }, { claudeAccount: "gone" }, list)).toEqual({
      id: "pro",
      source: "workspace",
      missing: "gone",
    });
    expect(list).toHaveBeenCalledTimes(2);
  });
  it("starts on main when the accounts cannot be listed, and says which was asked", async () => {
    const list = vi.fn(async (): Promise<ClaudeAccount[]> => {
      throw new Error("backend down");
    });
    expect(await launchAccount({ claudeAccount: "main" }, { claudeAccount: "pro" }, {}, list)).toEqual({
      id: "main",
      source: "default",
      missing: "pro",
    });
  });
});

describe("launched accounts", () => {
  it("remember the account each pane started on until it goes", () => {
    const seen = vi.fn();
    const stop = subscribeLaunchedAccounts(seen);
    expect(launchedAccount("pane-a")).toBeNull();
    noteLaunchedAccount("pane-a", "pro");
    noteLaunchedAccount("pane-b", "main");
    expect(launchedAccount("pane-a")).toBe("pro");
    expect(launchedAccount("pane-b")).toBe("main");
    // The same again tells nobody.
    noteLaunchedAccount("pane-a", "pro");
    expect(seen).toHaveBeenCalledTimes(2);
    noteLaunchedAccount("pane-a", null);
    expect(launchedAccount("pane-a")).toBeNull();
    expect(seen).toHaveBeenCalledTimes(3);
    stop();
    noteLaunchedAccount("pane-b", null);
    expect(seen).toHaveBeenCalledTimes(3);
  });
});
