"use strict";
// JSON's answer is JSON.parse's. YAML is scanned and TOML parsed for what is certainly
// wrong, so the first thing checked is that valid files, full of traps, pass.
const test = require("node:test");
const assert = require("node:assert/strict");
const { validate: check } = require("./helpers.js");
const { validate, checkerFor } = check;

test("which check applies", () => {
  const table = {
    "a.json": "json", "pkg/package.json": "json", "b.JSON": "json",
    "tsconfig.json": "jsonc", "tsconfig.app.json": "jsonc", ".vscode/settings.json": "jsonc", "c.jsonc": "jsonc", "w.code-workspace": "jsonc",
    "a.yaml": "yaml", "a.yml": "yaml", "a.toml": "toml", "Cargo.toml": "toml",
    "a.txt": null, "a.ini": null, "a.json5": null, Dockerfile: null,
  };
  for (const [path, kind] of Object.entries(table)) assert.equal(checkerFor(path), kind, path);
});

test("JSON: what JSON.parse says, with a line when the engine gives one", () => {
  assert.deepEqual(validate("a.json", '{"a": [1, 2]}'), { kind: "json", ok: true, partial: false });
  const bad = validate("a.json", '{\n  "a": 1,\n  "b": \n}\n');
  assert.equal(bad.ok, false);
  assert.equal(bad.partial, false);
  assert.equal(bad.code, "json");
  assert.equal(bad.line, 4);
  assert.equal(validate("a.json", "").ok, false, "an empty file is no JSON");
  assert.equal(validate("a.json", '{"a": 1} // no').ok, false);
  assert.equal(validate("a.json", '{"a": 1,}').ok, false);
});

test("JSON: where it goes wrong is found on any engine, and agrees with JSON.parse on what is wrong", () => {
  const cases = [
    ['{"a": 1,}', 1, 9],
    ['[1, 2\n', 2, 1],
    ['{\n"a" 1}', 2, 5],
    ['{"a": tru}', 1, 7],
    ['"\\q"', 1, 2],
    ['"new\nline"', 1, 5],
    ['1 2', 1, 3],
    ['{"a":01}', 1, 7],
    ['[1,\n2,\n,3]', 3, 1],
    ['nul', 1, 1],
  ];
  for (const [text, line, column] of cases) {
    assert.throws(() => JSON.parse(text), undefined, text);
    const result = validate("a.json", text);
    assert.equal(result.ok, false, text);
    assert.deepEqual([result.line, result.column], [line, column], text);
  }
  for (const text of ['"\\u00e9 \\" \\n"', '[[], {}, [{"a": [null, true, false, -1.5e+3, 0]}]]', " \n 1 \n"]) {
    assert.equal(validate("a.json", text).ok, true, text);
  }
  assert.equal(validate("a.json", "[".repeat(200000)).ok, false, "too deep to walk is still not valid");
});

test("JSONC: comments and trailing commas are fine, and strings are left alone", () => {
  const good = '// head\n{\n  /* block\n     over lines */\n  "url": "http://x.y/*z*/", // tail\n  "list": [1, 2,],\n  "o": { "a": "// not, a comment", },\n}\n';
  assert.equal(validate("tsconfig.json", good).ok, true);
  assert.equal(validate(".vscode/settings.json", good).ok, true);
  assert.equal(validate("a.json", good).ok, false, "plain JSON has neither");
  const bad = validate("c.jsonc", '{\n  // fine\n  "a": 1\n  "b": 2\n}\n');
  assert.equal(bad.ok, false);
  assert.equal(bad.line, 4, "the lines are still the file's");
  assert.equal(validate("c.jsonc", '{"a": ",", }').ok, true);
  assert.equal(validate("c.jsonc", "[1,,]").ok, false, "only the last comma goes");
});

const GOOD_YAML = {
  workflow: `name: CI
on:
  push:
    branches: [main, "release/**"]
env: { A: 1, B: "x,y" }
jobs:
  build:
    runs-on: \${{ matrix.os }}
    if: github.event_name == 'push' && contains(x, 'y')
    steps:
      - uses: actions/checkout@v4
      - name: Say "hi"   # a comment with a ' quote and a [ bracket
        run: |
          echo "unterminated
          it's [fine { here
          	tab inside a block scalar
      - run: >-
          folded 'text
        env:
          X: 'it''s'
          Y: "a \\" b"
`,
  plain: `key: it's fine
other: a "quoted" word
url: http://x.y:80/a#b
time: 12:30:00
list:
- a
- b: c
  d: [e, {f: g}]
- 'single'
- "double"
-   - nested
    - more
empty:
tilde: ~
glob: "*.js"
star: *anchor
amp: &anchor value
tagged: !!str "text"
tagged2: !custom [a, b]
? complex key
: complex value
`,
  documents: `%YAML 1.2
---
a: 1
...
--- # second
b: 2
--- |
  text with 'a quote
--- "quoted"
`,
  multiline: `a: "first line
  second 'line"
b: 'first
  second "line'
c: [1,
  2,
	3]
d: {
  e: f
}
plain: this goes on
  'over several lines
  and "another
next: 1
`,
  jsonish: '{"a": 1, "b": [true, null], "c": {"d": "e"}}\n',
  tabs: "a: 1\n\t\n  \t# a comment after a tab\nb: 2 \t\n",
  empty: "",
  crlf: "a: 1\r\nb:\r\n  - c\r\n",
};

for (const [name, text] of Object.entries(GOOD_YAML)) {
  test(`valid YAML passes: ${name}`, () => {
    assert.deepEqual(validate("a.yaml", text), { kind: "yaml", ok: true, partial: true });
  });
}

test("broken YAML: what is certainly wrong, with its line", () => {
  const cases = [
    ["a: 1\n\tb: 2\n", "yaml-tab", 2],
    ["a:\n  - b\n \t- c\n", "yaml-tab", 3],
    ['a: "never closed\nb: 2\n', "yaml-quote", 1],
    ["a: 1\nb: 'never closed\n", "yaml-quote", 2],
    ["a: [1, 2\nb: 3\n", "yaml-bracket", 1],
    ["a: {b: 1]\n", "yaml-bracket", 1],
    ["a: ]\n", "yaml-bracket", 1],
    ["a: @scope/pkg\n", "yaml-reserved", 1],
    ["a: `tick`\n", "yaml-reserved", 1],
  ];
  for (const [text, code, line] of cases) {
    const result = validate("a.yml", text);
    assert.equal(result.ok, false, JSON.stringify(text));
    assert.equal(result.code, code, JSON.stringify(text));
    assert.equal(result.line, line, JSON.stringify(text));
    assert.equal(result.partial, true);
  }
});

const GOOD_TOML = {
  everything: `# comment
title = "TOML" # trailing
"quoted key" = 1
'lit key' = 2
a.b.c = 3
d . e = 4
bare-key_1 = true
no = false
int = +1_000
hex = 0xDEAD_beef
oct = 0o755
bin = 0b1010
float = 6.626e-34
inf = -inf
nan = nan
date = 1979-05-27
dt = 1979-05-27T07:32:00Z
dt2 = 1979-05-27 07:32:00.5-07:00
time = 07:32:00
str = "esc \\" \\\\ \\t \\u00E9 \\U0001F600"
lit = 'C:\\no\\escape'
multi = """
line \\
  joined "quotes" ""
"""
multi2 = '''
raw 'x' ''
'''
arr = [1, 2, 3,]
arr2 = [
  "a", # comment
  [1, 2],

  { x = 1, y = "z" },
]
empty = []
inline = { a = 1, b.c = 2 }
none = {}

[table]
x = 1

[table.sub]
x = 2

[ spaced . "name" ]
x = 3

[[items]]
name = "one"

[[items]]
name = "two"

[other]
x = 4
`,
  crlf: 'a = 1\r\n# c\r\n[b]\r\nc = "d"\r\n',
  empty: "",
  "no final newline": "a = 1",
};

for (const [name, text] of Object.entries(GOOD_TOML)) {
  test(`valid TOML passes: ${name}`, () => {
    assert.deepEqual(validate("a.toml", text), { kind: "toml", ok: true, partial: true });
  });
}

test("broken TOML, with its line", () => {
  const cases = [
    ["a = \n", "toml-value", 1],
    ["ok = 1\nb\n", "toml-equals", 2],
    ["a = 1 2\n", "toml-trailing", 1],
    ['a = "open\n', "toml-string", 1],
    ['ok = 1\nb = "bad \\q"\n', "toml-escape", 2],
    ["= 1\n", "toml-key", 1],
    ["[table\nb = 1\n", "toml-header", 1],
    ["[[table]\n", "toml-header", 1],
    ["a = 1\na = 2\n", "toml-duplicate", 2],
    ["[t]\nx = 1\n[t]\ny = 2\n", "toml-duplicate", 3],
    ["[t]\nx = 1\nx = 2\n", "toml-duplicate", 3],
    ["a = [1, 2\n", "toml-value", 1],
    ["a = { b = 1,\n}\n", "toml-key", 1],
    ["a = nope\n", "toml-value", 1],
    ["a = 01\n", "toml-value", 1],
    ["a = 1_\n", "toml-value", 1],
    ["a = truex\n", "toml-value", 1],
    ["a = '''open\n", "toml-string", 1],
  ];
  for (const [text, code, line] of cases) {
    const result = validate("a.toml", text);
    assert.equal(result.ok, false, JSON.stringify(text));
    assert.equal(result.code, code, JSON.stringify(text));
    assert.equal(result.line, line, JSON.stringify(text));
  }
});

test("the same key in two tables, or two [[items]], is no duplicate", () => {
  assert.equal(validate("a.toml", "[a]\nx = 1\n[b]\nx = 2\n").ok, true);
  assert.equal(validate("a.toml", "[[a]]\nx = 1\n[[a]]\nx = 2\n").ok, true);
});

test("the repository's own files pass, when they are around", () => {
  const fs = require("node:fs");
  const path = require("node:path");
  const root = path.join(__dirname, "..", "..", "..");
  const samples = [".github/workflows/ci.yml", ".github/workflows/release.yml", ".github/ISSUE_TEMPLATE/bug_report.yml", "pnpm-lock.yaml", "src-tauri/Cargo.toml", "sws.toml", "package.json", "tsconfig.json"];
  for (const sample of samples) {
    const file = path.join(root, sample);
    if (!fs.existsSync(file)) continue; // a kit installed on its own has none beside it
    const result = validate(sample, fs.readFileSync(file, "utf8"));
    assert.equal(result.ok, true, `${sample}: ${JSON.stringify(result)}`);
  }
});

test("no check for other files: nothing can be said, so nothing is claimed", () => {
  assert.deepEqual(validate("a.txt", "whatever {{{"), { kind: null, ok: true, partial: false });
});
