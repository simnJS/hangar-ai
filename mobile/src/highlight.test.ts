import { describe, expect, it } from "vitest";
import { highlight, type Language, type TokenKind } from "./highlight";

/** The coloured pieces, without the plain ones. */
const coloured = (source: string, language: Language) =>
  highlight(source, language)
    .filter((token) => token.kind !== "plain")
    .map((token): [TokenKind, string] => [token.kind, token.text]);

const SAMPLES: [Language, string][] = [
  ["js", 'const a = `x ${y}\nz`; // done\n/* block\n comment */ if (a) return "q\\"uote";'],
  ["rust", 'fn main() { let s = "é"; /* c */ println!("{}", 0x1F); }'],
  ["python", 'def f():\n    """doc\n    string"""\n    return 3.5  # note'],
  ["shell", "echo $HOME#not # a comment\nexport A='b'"],
  ["sql", "SELECT id FROM t -- why\nWHERE a = 'b';"],
  ["css", ".a:hover { color: #fff; } /* c */ @media (x) {}"],
  ["json", '{"key": [1, true, null, "v"]}'],
  ["yaml", "name: hangar # app\nversion: 1.2.0\nlist:\n  - one"],
  ["toml", '[package]\nname = "hangar"\nversion = "1.2.0"'],
  ["markup", '<!doctype html><!-- c --><a href="x" data-y=z disabled>text</a><br/>'],
  ["lua", "--[[ long\ncomment ]] local x = [[s]] -- end"],
  ["verse", "<# block #> using { /Verse.org } # c\nvar X : int = 0"],
  ["batch", "@echo off\nREM a comment\n:: another\nset A=\"b\""],
  ["plain", "anything at all\n"],
];

describe("highlighting", () => {
  it("never loses or adds a character: the pieces end to end are the source", () => {
    for (const [language, source] of SAMPLES) {
      expect(highlight(source, language).map((token) => token.text).join(""), language).toBe(source);
    }
  });

  it("colours a script's comments, strings, numbers and keywords", () => {
    expect(coloured('const n = 42; // why\nlet s = "a // b";', "js")).toEqual([
      ["keyword", "const"],
      ["number", "42"],
      ["comment", "// why"],
      ["keyword", "let"],
      ["string", '"a // b"'],
    ]);
  });

  it("keeps a template string over its lines, and an escaped quote inside a string", () => {
    expect(coloured("`one\ntwo`", "js")).toEqual([["string", "`one\ntwo`"]]);
    expect(coloured('"a\\"b" c', "js")).toEqual([["string", '"a\\"b"']]);
    // A one-line string stops at the line's end, unclosed.
    expect(coloured('"open\nnext', "js")).toEqual([["string", '"open']]);
  });

  it("colours a page's tags, attributes, values and comments", () => {
    expect(coloured('<a href="x" hidden>hi</a><!-- c -->', "markup")).toEqual([
      ["tag", "<a"],
      ["attr", "href"],
      ["string", '"x"'],
      ["attr", "hidden"],
      ["tag", ">"],
      ["tag", "</a"],
      ["tag", ">"],
      ["comment", "<!-- c -->"],
    ]);
  });

  it("tells a key from a value", () => {
    expect(coloured('{"name": "hangar"}', "json")).toEqual([
      ["attr", '"name"'],
      ["string", '"hangar"'],
    ]);
    expect(coloured("name: hangar\nport: 21000", "yaml")).toEqual([
      ["attr", "name"],
      ["attr", "port"],
      ["number", "21000"],
    ]);
  });

  it("reads a lone # as a comment only where one can start", () => {
    expect(coloured("echo a#b # real", "shell")).toEqual([
      ["keyword", "echo"],
      ["comment", "# real"],
    ]);
  });

  it("reads SQL's keywords in any case", () => {
    expect(coloured("select 1 FROM t", "sql")).toEqual([
      ["keyword", "select"],
      ["number", "1"],
      ["keyword", "FROM"],
    ]);
  });

  it("runs an unclosed comment or string to the end, and plain text stays one piece", () => {
    expect(coloured("a /* never closed", "js")).toEqual([["comment", "/* never closed"]]);
    expect(highlight("<b>unclosed", "markup").map((token) => token.text).join("")).toBe("<b>unclosed");
    expect(highlight("just words\n", "plain")).toEqual([{ kind: "plain", text: "just words\n" }]);
    expect(highlight("", "js")).toEqual([]);
  });

  it("colours a long file in one pass", () => {
    const source = 'const value = "text"; // a comment\n'.repeat(4000);
    const started = performance.now();
    const tokens = highlight(source, "js");
    expect(performance.now() - started).toBeLessThan(1000);
    expect(tokens.filter((token) => token.kind === "comment")).toHaveLength(4000);
  });
});
