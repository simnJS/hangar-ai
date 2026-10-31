import { describe, expect, it } from "vitest";
import { readAccountChoice, resolveAccount, type ClaudeAccount } from "./accounts";

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
