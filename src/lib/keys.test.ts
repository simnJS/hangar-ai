import { describe, expect, it } from "vitest";
import {
  canonicalKey,
  chordFromEvent,
  formatBinding,
  isBareChord,
  isMac,
  isTextEditingChord,
  keyFromEvent,
  parseBinding,
  parseChord,
  serializeBinding,
  writeBinding,
  type Chord,
} from "./keys";

const chord = (key: string, mods: Partial<Omit<Chord, "key">> = {}): Chord => ({
  key,
  ctrl: false,
  shift: false,
  alt: false,
  meta: false,
  ...mods,
});

/** What `Mod` stands for on the platform the tests run on. */
const MOD = isMac() ? { meta: true } : { ctrl: true };

/** Just the fields keyFromEvent and chordFromEvent read. */
function keyEvent(
  key: string,
  code: string,
  mods: { ctrl?: boolean; shift?: boolean; alt?: boolean; meta?: boolean; altGraph?: boolean } = {},
): KeyboardEvent {
  return {
    key,
    code,
    ctrlKey: !!mods.ctrl,
    shiftKey: !!mods.shift,
    altKey: !!mods.alt,
    metaKey: !!mods.meta,
    getModifierState: (name: string) => name === "AltGraph" && !!mods.altGraph,
  } as unknown as KeyboardEvent;
}

describe("canonicalKey", () => {
  it("folds every spelling onto one name", () => {
    expect(canonicalKey("esc")).toBe("Escape");
    expect(canonicalKey("Return")).toBe("Enter");
    expect(canonicalKey("ArrowUp")).toBe("Up");
    expect(canonicalKey("pgdn")).toBe("PageDown");
    expect(canonicalKey("comma")).toBe(",");
  });

  it("upper-cases letters and function keys", () => {
    expect(canonicalKey("a")).toBe("A");
    expect(canonicalKey("f5")).toBe("F5");
    expect(canonicalKey("F12")).toBe("F12");
  });

  it("is null for nothing at all", () => {
    expect(canonicalKey("   ")).toBeNull();
  });
});

describe("parseChord", () => {
  it("reads modifiers in any order and case", () => {
    expect(parseChord("Ctrl+Shift+Enter")).toEqual(chord("Enter", { ctrl: true, shift: true }));
    expect(parseChord("shift+ctrl+enter")).toEqual(chord("Enter", { ctrl: true, shift: true }));
  });

  it("accepts the platform names of the modifiers", () => {
    expect(parseChord("Cmd+Option+X")).toEqual(chord("X", { meta: true, alt: true }));
    expect(parseChord("Control+Win+Q")).toEqual(chord("Q", { ctrl: true, meta: true }));
  });

  it("reads Mod as the platform modifier", () => {
    expect(parseChord("Mod+K")).toEqual(chord("K", MOD));
  });

  it("takes a plus sign as the key itself", () => {
    expect(parseChord("Ctrl++")).toEqual(chord("+", { ctrl: true }));
    expect(parseChord("Ctrl+=")).toEqual(chord("=", { ctrl: true }));
  });

  it("refuses a lone modifier", () => {
    expect(parseChord("Ctrl")).toBeNull();
    expect(parseChord("Ctrl+Shift")).toBeNull();
    expect(parseChord("Mod")).toBeNull();
  });

  it("refuses an unknown word where a modifier belongs", () => {
    expect(parseChord("Hyper+A")).toBeNull();
  });

  it("refuses an empty string", () => {
    expect(parseChord("")).toBeNull();
  });
});

describe("parseBinding", () => {
  it("reads a two-step sequence", () => {
    expect(parseBinding("Ctrl+K Ctrl+S")).toEqual([
      chord("K", { ctrl: true }),
      chord("S", { ctrl: true }),
    ]);
  });

  it("is null as soon as one step does not parse", () => {
    expect(parseBinding("Ctrl+K Hyper+S")).toBeNull();
    expect(parseBinding("  ")).toBeNull();
  });
});

describe("serializeBinding", () => {
  it("gives two spellings of the same keys the same form", () => {
    expect(serializeBinding("Shift+Ctrl+a")).toBe("ctrl+shift+A");
    expect(serializeBinding("ctrl+shift+A")).toBe("ctrl+shift+A");
    expect(serializeBinding("Meta+Shift+Alt+Ctrl+Up")).toBe("ctrl+alt+shift+meta+Up");
  });

  it("keeps the steps of a sequence apart", () => {
    expect(serializeBinding("Ctrl+K  Ctrl+S")).toBe("ctrl+K ctrl+S");
  });

  it("is null for what does not parse", () => {
    expect(serializeBinding("Hyper+A")).toBeNull();
  });
});

describe("writing a binding back", () => {
  it("round-trips through the parser", () => {
    const chords = [chord("K", { ctrl: true, alt: true }), chord("Enter", { shift: true })];
    expect(writeBinding(chords)).toBe("Ctrl+Alt+K Shift+Enter");
    expect(parseBinding(writeBinding(chords))).toEqual(chords);
  });

  it("formats for the eye, leaving what does not parse as it is", () => {
    expect(formatBinding("Hyper+A")).toBe("Hyper+A");
    if (!isMac()) {
      expect(formatBinding("ctrl+k ctrl+s")).toBe("Ctrl+K Ctrl+S");
      expect(formatBinding("Alt+Up")).toBe("Alt+↑");
    }
  });
});

describe("keyFromEvent", () => {
  it("upper-cases letters", () => {
    expect(keyFromEvent(keyEvent("a", "KeyA"))).toBe("A");
  });

  it("keeps the printed letter on a layout that moves it", () => {
    // AZERTY: the key printed "A" sits where QWERTY has Q.
    expect(keyFromEvent(keyEvent("a", "KeyQ"))).toBe("A");
  });

  it("falls back to the digit when the digit row needs Shift", () => {
    // AZERTY: the "1" key types "&" without Shift.
    expect(keyFromEvent(keyEvent("&", "Digit1"))).toBe("1");
  });

  it("falls back to the code on a non-Latin layout", () => {
    expect(keyFromEvent(keyEvent("ф", "KeyA"))).toBe("A");
  });

  it("names the numpad symbols and the arrows", () => {
    expect(keyFromEvent(keyEvent("+", "NumpadAdd"))).toBe("+");
    expect(keyFromEvent(keyEvent("Enter", "NumpadEnter"))).toBe("Enter");
    expect(keyFromEvent(keyEvent("ArrowLeft", "ArrowLeft"))).toBe("Left");
    expect(keyFromEvent(keyEvent(" ", "Space"))).toBe("Space");
  });

  it("is null for a modifier on its own", () => {
    expect(keyFromEvent(keyEvent("Shift", "ShiftLeft"))).toBeNull();
    expect(keyFromEvent(keyEvent("Control", "ControlLeft"))).toBeNull();
  });
});

describe("chordFromEvent", () => {
  it("carries the modifiers held", () => {
    expect(chordFromEvent(keyEvent("k", "KeyK", { ctrl: true, shift: true }))).toEqual(
      chord("K", { ctrl: true, shift: true }),
    );
  });

  it("ignores a character composed with AltGr", () => {
    // Windows reports AltGr as Ctrl+Alt; on AZERTY that is how "@" is typed.
    expect(chordFromEvent(keyEvent("@", "Digit0", { ctrl: true, alt: true, altGraph: true }))).toBeNull();
    expect(chordFromEvent(keyEvent("0", "Digit0", { ctrl: true, alt: true }))).toEqual(
      chord("0", { ctrl: true, alt: true }),
    );
  });
});

describe("chord classes", () => {
  it("calls a chord bare when nothing keeps it out of typing", () => {
    expect(isBareChord(chord("A", { shift: true }))).toBe(true);
    expect(isBareChord(chord("A", { alt: true }))).toBe(false);
  });

  it("recognises the editing chords a text field owns", () => {
    expect(isTextEditingChord(chord("C", MOD))).toBe(true);
    expect(isTextEditingChord(chord("V", MOD))).toBe(true);
    expect(isTextEditingChord(chord("K", MOD))).toBe(false);
    expect(isTextEditingChord(chord("C", { ...MOD, alt: true }))).toBe(false);
    expect(isTextEditingChord(chord("C"))).toBe(false);
  });
});
