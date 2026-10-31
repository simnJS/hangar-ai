import { describe, expect, it } from "vitest";
import type { ClaudeAccount } from "../../lib/accounts";
import { accountMenuItems } from "./accountMenuItems";
import type { AccountsT } from "./strings";

const accounts: ClaudeAccount[] = [
  { id: "main", label: "Principal", isMain: true, loggedIn: true },
  { id: "perso", label: "Perso", isMain: false, loggedIn: true, email: "me@example.com" },
];
const t: AccountsT = (key) => (key === "picker.default" ? "Default ({name})" : "unknown");

describe("accountMenuItems", () => {
  it("lists the default first and checks it when the workspace has no account", () => {
    const items = accountMenuItems({ accounts, settings: { claudeAccount: "perso" }, onSelect: () => {}, t });
    expect(items.map((i) => [i.label, i.checked])).toEqual([
      ["Default (Perso)", true],
      ["Principal", false],
      ["Perso", false],
    ]);
  });

  it("checks the workspace's own account, and reports the choice", () => {
    const picked: (string | null)[] = [];
    const items = accountMenuItems({ accounts, value: "perso", onSelect: (id) => picked.push(id), t });
    expect(items.map((i) => i.checked)).toEqual([false, false, true]);
    items[0].onSelect();
    items[1].onSelect();
    expect(picked).toEqual([null, "main"]);
  });

  it("treats a dead id as the default and never prints an email", () => {
    const items = accountMenuItems({ accounts, value: "gone", onSelect: () => {}, t });
    expect(items[0].checked).toBe(true);
    expect(items.some((i) => i.label.includes("@"))).toBe(false);
  });
});
