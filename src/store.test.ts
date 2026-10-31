import { describe, expect, it } from "vitest";
import { makeWorkspace } from "./store";

describe("makeWorkspace and the Claude account", () => {
  const base = { name: "w", cwd: "/p", layout: 1 as const, agents: ["claude" as const], shellId: null };

  it("keeps the account the dialog chose", () => {
    expect(makeWorkspace({ ...base, claudeAccount: "travail" }).claudeAccount).toBe("travail");
  });

  it("leaves the field out when the workspace follows the default", () => {
    expect("claudeAccount" in makeWorkspace(base)).toBe(false);
    expect("claudeAccount" in makeWorkspace({ ...base, claudeAccount: null })).toBe(false);
    expect("claudeAccount" in makeWorkspace({ ...base, claudeAccount: "  " })).toBe(false);
  });
});
