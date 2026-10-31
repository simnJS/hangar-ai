"use strict";
// Comments, order and every untouched byte survive a save; a value is a value, even
// over several lines, and is never read as a key.
const test = require("node:test");
const assert = require("node:assert/strict");
const { dotenv } = require("./helpers.js");

const value = (doc, key) => dotenv.rows(doc).find((row) => row.key === key)?.value;
const edit = (text, edits) => dotenv.serialize(dotenv.withValues(dotenv.parse(text), new Map(Object.entries(edits))));

const SHAPES = {
  empty: "",
  "one line, no end of line": "A=1",
  plain: "# head\n\nA=1\nB=2\n",
  crlf: "# head\r\n\r\nA=1\r\nB=2\r\n",
  "mixed endings": "A=1\r\nB=2\nC=3\r\n",
  "lone CR": "A=1\rB=2\r",
  exported: "export A=1\n  export   B = 2\n",
  spaced: "A = 1\nB= 2\nC =3\n\tD=4\n",
  quotes: "A=\"1\"\nB='2'\nC=`3`\nD=\"\"\nE=''\n",
  "inline comments": "A=1 # one\nB=\"2\" # two\nC= # three\nD=a#b\n",
  "several lines": "A=\"one\ntwo\nB=inside\"\nC='x\ny=z'\nD=1\n",
  "CRLF inside a value": "A='one\r\ntwo'\r\nB=2\r\n",
  "unclosed quotes": "A=\"open\nB=2\nC='open\nD=4\n",
  "escaped quotes": "A=\"say \\\"hi\\\"\"\nB='it\\'s'\nC=\"back\\\\\"\n",
  "not understood": "just-a-token\nA=1\n1BAD=2\nKEY: value\n=lonely\n",
  duplicates: "A=1\n# again\nA=2\nB=3\nA=4\n",
  "blank lines at the end": "A=1\n\n\n",
  "trailing blanks": "A=1   \nB=2\t\n",
  unicode: "NOM=Éléonore\nÉTÉ=chaud\nA=✓\n",
  "equals in a value": "A=b=c==\nURL=https://x.y/?a=1&b=2\n",
};

for (const [name, text] of Object.entries(SHAPES)) {
  test(`read and written back unchanged: ${name}`, () => {
    assert.equal(dotenv.serialize(dotenv.parse(text)), text);
    assert.equal(edit(text, {}), text);
  });
}

test("values are read as parsers read them", () => {
  const doc = dotenv.parse(SHAPES.quotes + SHAPES["inline comments"].replace(/^A/gm, "F").replace(/^B/gm, "G").replace(/^C/gm, "H").replace(/^D/gm, "I"));
  assert.equal(value(doc, "A"), "1");
  assert.equal(value(doc, "B"), "2");
  assert.equal(value(doc, "C"), "3");
  assert.equal(value(doc, "D"), "");
  assert.equal(value(doc, "E"), "");
  assert.equal(value(doc, "F"), "1", "a comment after a blank is no part of the value");
  assert.equal(value(doc, "G"), "2");
  assert.equal(value(doc, "H"), "", "nothing but a comment");
  assert.equal(value(doc, "I"), "a#b", "a # inside a word is part of it");
  assert.equal(value(dotenv.parse(SHAPES.exported), "B"), "2");
  assert.equal(value(dotenv.parse(SHAPES.spaced), "D"), "4");
  assert.equal(value(dotenv.parse(SHAPES["equals in a value"]), "A"), "b=c==");
  assert.equal(value(dotenv.parse(SHAPES["escaped quotes"]), "A"), 'say "hi"');
  assert.equal(value(dotenv.parse(SHAPES["escaped quotes"]), "B"), "it's");
  assert.equal(value(dotenv.parse(SHAPES["escaped quotes"]), "C"), "back\\");
});

test("what is inside a multi-line value is no key, and an unclosed quote swallows nothing", () => {
  const several = dotenv.parse(SHAPES["several lines"]);
  assert.deepEqual(dotenv.rows(several).map((row) => row.key), ["A", "C", "D"], "B=inside and y=z belong to values");
  assert.equal(value(several, "A"), "one\ntwo\nB=inside");
  assert.equal(value(several, "C"), "x\ny=z");
  const unclosed = dotenv.parse(SHAPES["unclosed quotes"]);
  assert.deepEqual(dotenv.rows(unclosed).map((row) => row.key), ["A", "B", "C", "D"]);
  assert.equal(value(unclosed, "A"), '"open');
});

test("a line that is no pair is no key, and is counted to say so", () => {
  const doc = dotenv.parse(SHAPES["not understood"]);
  assert.deepEqual(dotenv.rows(doc).map((row) => row.key), ["A"]);
  assert.equal(dotenv.otherLines(doc), 4);
  assert.equal(dotenv.otherLines(dotenv.parse("# comment\n\nA=1\n")), 0);
});

test("setting a value changes that line and no other", () => {
  assert.equal(edit("# head\n\nA=1\n\n# about b\nB=2 # inline\n# tail\n", { B: "new" }), "# head\n\nA=1\n\n# about b\nB=new # inline\n# tail\n");
  assert.equal(edit("export A=1\n", { A: "2" }), "export A=2\n");
  assert.equal(edit("  A = 1\n", { A: "2" }), "  A = 2\n");
  assert.equal(edit('A="1"\n', { A: "2" }), 'A="2"\n', "its quotes are kept when nothing needs them changed");
  assert.equal(edit("A='1'\r\nB=2\r\n", { A: "x" }), "A='x'\r\nB=2\r\n");
  assert.equal(edit("A= # empty\n", { A: "x" }), "A= x # empty\n");
  assert.equal(edit('A="one\ntwo" # c\nB=1\n', { A: "x" }), 'A="x" # c\nB=1\n');
  assert.equal(edit("A=1\n", { A: "1" }), "A=1\n");
});

test("a new key goes at the end, in the file's line endings, and nothing moves", () => {
  assert.equal(edit("A=1\n", { Z: "9" }), "A=1\nZ=9\n");
  assert.equal(edit("A=1", { Z: "9" }), "A=1\nZ=9\n", "the last line gets its end of line first");
  assert.equal(edit("A=1\r\nB=2\r\n", { Z: "9" }), "A=1\r\nB=2\r\nZ=9\r\n");
  assert.equal(edit("", { Z: "9" }), "Z=9\n");
  assert.equal(edit("# only a comment\n", { Z: "9", Y: "8" }), "# only a comment\nZ=9\nY=8\n");
});

test("a key defined twice shows once, and the last line is the one read and edited", () => {
  const doc = dotenv.parse(SHAPES.duplicates);
  const a = dotenv.rows(doc).find((row) => row.key === "A");
  assert.equal(a.value, "4");
  assert.equal(a.count, 3);
  assert.equal(edit(SHAPES.duplicates, { A: "x" }), "A=1\n# again\nA=2\nB=3\nA=x\n");
});

test("any value is read back the same once written", () => {
  const values = [
    "plain", "with space", " leading", "trailing ", "#hash", "a #b", "it's", 'say "hi"', "both ' and \"", "back\\slash", "ends\\",
    "$HOME", "${HOME}", "`tick`", "two\nlines", "two\r\nlines", "it's\ntwo", "a=b", "é✓", "=", "'", '"', "\\", "\\n", "'\\n'", "tab\there",
  ];
  for (const original of ["A=1\n", 'A="1"\n', "A='1'\n", "export A = 1 # c\n"]) {
    for (const wanted of values) {
      const text = edit(original, { A: wanted, B: wanted });
      const doc = dotenv.parse(text);
      assert.equal(value(doc, "A"), wanted, `${JSON.stringify(wanted)} over ${JSON.stringify(original)}: ${JSON.stringify(text)}`);
      assert.equal(value(doc, "B"), wanted, `${JSON.stringify(wanted)} as a new key: ${JSON.stringify(text)}`);
      assert.deepEqual(dotenv.rows(doc).map((row) => row.key), ["A", "B"], JSON.stringify(text));
    }
  }
});

test("the comment above a key is its hint, but a value put aside in a comment is not", () => {
  const rows = dotenv.rows(dotenv.parse("# Get one at the dashboard\n# (free)\nA=1\n\n# unrelated\n\nB=2\n# OLD_TOKEN=abc123\n#export C=1\nC=3\n"));
  assert.equal(rows[0].hint, "Get one at the dashboard (free)");
  assert.equal(rows[1].hint, "", "a blank line cuts the link");
  assert.equal(rows[2].hint, "");
});

test("which keys changed: names only", () => {
  const changed = dotenv.changedKeys(dotenv.parse("A=1\nB=2\nC=\nD=4\n"), dotenv.parse("A=1\nB=x\nC=y\nD=\nE=z\nF=\n"));
  assert.deepEqual(changed, { set: ["B", "C", "E"], emptied: ["D"] });
});

test("what counts as a dotenv file, and what as a template", () => {
  for (const path of [".env", ".env.local", "config/.env.production", "app.env", "a/b/prod.ENV", ".env.example"]) {
    assert.ok(dotenv.isDotenvPath(path), path);
  }
  for (const path of [".envrc", "env", "env.json", "dotenv", ".environment", "config.yaml", ".env/file"]) {
    assert.ok(!dotenv.isDotenvPath(path), path);
  }
  assert.ok(dotenv.isExamplePath("sub/.env.example"));
  assert.ok(dotenv.isExamplePath(".env.sample"));
  assert.ok(!dotenv.isExamplePath(".env"));
  assert.ok(!dotenv.isExamplePath(".env.local"));
  assert.deepEqual(dotenv.exampleCandidates("sub/.env.local").slice(0, 3), ["sub/.env.local.example", "sub/.env.local.sample", "sub/.env.example"]);
  assert.ok(!dotenv.exampleCandidates(".env.example").includes(".env.example"));
});
