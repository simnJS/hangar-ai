import { afterEach, describe, expect, it, vi } from "vitest";
import { isMac, parseBinding, serializeBinding } from "./keys";
import {
  COMMANDS,
  COMMANDS_BY_ID,
  commandsFor,
  findConflicts,
  indexKeymap,
  isCustomized,
  resetCommand,
  resolveKeymap,
  shortcutLabel,
  withBinding,
  withoutBinding,
  type Keymap,
} from "./shortcuts";

/** How `Mod+…` serializes on the platform the tests run on. */
const MOD = isMac() ? "meta" : "ctrl";

describe("the command table", () => {
  it("has unique ids", () => {
    expect(COMMANDS_BY_ID.size).toBe(COMMANDS.length);
  });

  it("ships only defaults that parse", () => {
    for (const command of COMMANDS) {
      for (const binding of command.defaults) {
        expect(parseBinding(binding), `${command.id}: ${binding}`).not.toBeNull();
      }
    }
  });

  it("ships no two commands on the same keys", () => {
    expect(findConflicts(resolveKeymap({}))).toEqual([]);
  });

  describe("on a Mac", () => {
    afterEach(() => {
      vi.unstubAllGlobals();
      vi.resetModules();
    });

    it("ships no two commands on the same keys either", async () => {
      // keys.ts reads the platform once, when it loads: load it again as a Mac.
      vi.stubGlobal("navigator", { platform: "MacIntel", userAgent: "Macintosh" });
      vi.resetModules();
      const keys = await import("./keys");
      const mac = await import("./shortcuts");
      expect(keys.isMac()).toBe(true);
      expect(mac.findConflicts(mac.resolveKeymap({}))).toEqual([]);
    });
  });
});

describe("resolveKeymap", () => {
  it("fills in the defaults", () => {
    const keymap = resolveKeymap({});
    for (const command of COMMANDS) expect(keymap[command.id]).toEqual(command.defaults);
  });

  it("lets an override replace them, and an empty list turn a command off", () => {
    const keymap = resolveKeymap({ "pane.split": ["F9"], "pane.close": [] });
    expect(keymap["pane.split"]).toEqual(["F9"]);
    expect(keymap["pane.close"]).toEqual([]);
  });

  it("survives settings that were never written", () => {
    expect(resolveKeymap(null)["pane.split"]).toEqual(COMMANDS_BY_ID.get("pane.split")!.defaults);
  });
});

describe("editing bindings", () => {
  const defaults = COMMANDS_BY_ID.get("pane.split")!.defaults;

  it("adds a binding next to the defaults", () => {
    const next = withBinding({}, resolveKeymap({}), "pane.split", "F9");
    expect(next).toEqual({ "pane.split": [...defaults, "F9"] });
    expect(isCustomized(next, "pane.split")).toBe(true);
  });

  it("ignores a binding the command already answers to, however it is spelled", () => {
    const overrides: Keymap = {};
    const respelled = defaults[0].toLowerCase();
    expect(serializeBinding(respelled)).toBe(serializeBinding(defaults[0]));
    expect(withBinding(overrides, resolveKeymap(overrides), "pane.split", respelled)).toBe(overrides);
  });

  it("ignores a binding that does not parse", () => {
    const overrides: Keymap = {};
    expect(withBinding(overrides, resolveKeymap(overrides), "pane.split", "Hyper+A")).toBe(overrides);
  });

  it("forgets the override once it matches the defaults again", () => {
    const added = withBinding({}, resolveKeymap({}), "pane.split", "F9");
    const removed = withoutBinding(added, resolveKeymap(added), "pane.split", "F9");
    expect(removed).toEqual({});
  });

  it("keeps an emptied command as an override, since that is how it is turned off", () => {
    const keymap = resolveKeymap({});
    let overrides: Keymap = {};
    for (const binding of defaults) {
      overrides = withoutBinding(overrides, resolveKeymap(overrides), "pane.split", binding);
    }
    expect(overrides).toEqual({ "pane.split": [] });
    expect(keymap["pane.split"]).toEqual(defaults);
  });

  it("keeps bindings for commands this version does not know", () => {
    const overrides: Keymap = { "future.command": ["Ctrl+Q"] };
    const next = withBinding(overrides, resolveKeymap(overrides), "pane.split", "F9");
    expect(next["future.command"]).toEqual(["Ctrl+Q"]);
  });

  it("puts a command back on its defaults", () => {
    const next = resetCommand({ "pane.split": ["F9"], "pane.close": [] }, "pane.split");
    expect(next).toEqual({ "pane.close": [] });
    expect(isCustomized(next, "pane.split")).toBe(false);
  });
});

describe("indexKeymap", () => {
  it("indexes each sequence and the prefixes the dispatcher waits on", () => {
    const { exact, prefixes } = indexKeymap(resolveKeymap({}));
    expect(exact.get(`${MOD}+K ${MOD}+S`)).toEqual(["view.shortcuts"]);
    expect(prefixes.has(`${MOD}+K`)).toBe(true);
    expect(exact.has(`${MOD}+K`)).toBe(false);
  });

  it("skips a binding that no longer parses", () => {
    const { exact } = indexKeymap(resolveKeymap({ "pane.split": ["Hyper+A", "F9"] }));
    expect(exact.get("F9")).toEqual(["pane.split"]);
  });
});

describe("findConflicts", () => {
  it("reports two commands on one keystroke", () => {
    const keymap = resolveKeymap({ "pane.split": ["F9"], "pane.close": ["f9"] });
    expect(findConflicts(keymap)).toEqual([
      { kind: "duplicate", binding: "F9", commands: ["pane.split", "pane.close"] },
    ]);
  });

  it("reports a sequence whose first step is already a shortcut", () => {
    const keymap = resolveKeymap({ "pane.close": ["Mod+K"] });
    expect(findConflicts(keymap)).toEqual([
      { kind: "shadow", binding: "Mod+K Mod+S", commands: ["pane.close", "view.shortcuts"] },
    ]);
  });
});

describe("commandsFor", () => {
  it("names the owners of a keystroke, leaving out the one being edited", () => {
    const keymap = resolveKeymap({ "pane.split": ["F9"], "pane.close": ["F9"] });
    const chords = parseBinding("F9")!;
    expect(commandsFor(keymap, chords)).toEqual(["pane.split", "pane.close"]);
    expect(commandsFor(keymap, chords, "pane.split")).toEqual(["pane.close"]);
    expect(commandsFor(keymap, parseBinding("F10")!)).toEqual([]);
  });
});

describe("shortcutLabel", () => {
  it("is the first binding, formatted, and empty when unbound", () => {
    const keymap = resolveKeymap({ "pane.split": ["Ctrl+Shift+Enter", "F9"], "pane.close": [] });
    expect(shortcutLabel(keymap, "pane.close")).toBe("");
    if (!isMac()) expect(shortcutLabel(keymap, "pane.split")).toBe("Ctrl+Shift+Enter");
  });
});
